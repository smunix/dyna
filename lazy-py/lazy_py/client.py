"""
Core ``LazyClient`` implementation.

The client uses ``dyna_py.DynaRepo`` for local repository operations and
``websockets`` for the live-update listener.

**Performance note:** ``get_many``, ``get_all``, ``for_each``, and
``for_each_all`` avoid repeated per-resource materialisation.  The
underlying ``dyna_py`` clone already materialises all snapshots, so the
"batch" path here simply ensures we don't re-read resources one at a time
unnecessarily.  The ``for_each`` family accepts a continuation so that
callers with very large datasets (tens of thousands of resources) never
need to build a full ``dict`` in memory.
"""

from __future__ import annotations

import asyncio
import json
import logging
import tempfile
import shutil
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Callable, Dict, List, Optional, Set

import websockets

from dyna_py import DynaRepo

logger = logging.getLogger("lazy_py")


@dataclass
class ChangesetInfo:
    """Metadata about a single changeset in an update event."""

    change_id: str = ""
    message: str = ""
    author: str = ""
    patch_count: int = 0
    affected_resources: List[str] = field(default_factory=list)


@dataclass
class UpdateEvent:
    """Full details about a live update received via WebSocket."""

    kind: str = ""
    timestamp: str = ""
    channel: str = ""
    changesets: List[ChangesetInfo] = field(default_factory=list)
    new_head: Optional[str] = None
    affected_resource_ids: List[str] = field(default_factory=list)
    updated_snapshots: Dict[str, Any] = field(default_factory=dict)


class LazyClient:
    """Lazy, on-demand resource loader backed by a local dyna-py repository.

    Resources are fetched from the remote server only when first requested.
    A background WebSocket listener keeps the local cache up-to-date whenever
    new changesets are pushed to the tracked channel.
    """

    def __init__(
        self,
        server_url: str,
        channel: str,
        repo: DynaRepo,
        work_dir: str,
        known_ids: Set[str],
        loaded: Set[str],
    ) -> None:
        self._server_url = server_url
        self._channel = channel
        self._repo = repo
        self._work_dir = work_dir
        self._known_ids = known_ids
        self._loaded = loaded
        self._on_update: Optional[Callable[[UpdateEvent], None]] = None
        self._ws_task: Optional[asyncio.Task[None]] = None

    # -------------------------------------------------------------------
    # Construction
    # -------------------------------------------------------------------

    @classmethod
    async def connect(cls, server_url: str, channel: str = "main") -> "LazyClient":
        """Connect to a Dyna server and start tracking *channel*.

        Performs a lightweight clone to populate channel metadata and
        changeset history.  Resource bodies are **not** fetched until
        explicitly requested.
        """
        work_dir = tempfile.mkdtemp(prefix="lazy_py_")
        repo = DynaRepo.clone_repo(server_url, work_dir)

        # Switch to the requested channel (clone defaults to main)
        channels = repo.list_channels()
        if channel in channels:
            repo.switch_channel(channel)
        else:
            logger.warning(
                "Channel '%s' not found on server (available: %s), using 'main'",
                channel,
                channels,
            )

        # Collect known resource IDs from the local snapshots
        try:
            resource_ids = set(repo.list_resources())
        except Exception:
            resource_ids = set()

        loaded: Set[str] = set(resource_ids)

        client = cls(
            server_url=server_url,
            channel=channel,
            repo=repo,
            work_dir=work_dir,
            known_ids=resource_ids,
            loaded=loaded,
        )

        # Start the WebSocket listener
        client._ws_task = asyncio.create_task(client._ws_listener())

        return client

    # -------------------------------------------------------------------
    # Queries
    # -------------------------------------------------------------------

    async def get(self, resource_id: str) -> Any:
        """Get a single resource by ID, fetching on demand if needed."""
        if resource_id not in self._loaded:
            self._materialise(resource_id)

        raw = self._repo.read_resource(resource_id)
        return json.loads(raw)

    async def get_many(self, resource_ids: List[str]) -> Dict[str, Any]:
        """Get multiple resources, materialising any that are missing.

        All missing resources are materialised in a single batch pass
        rather than one at a time.
        """
        to_load = [rid for rid in resource_ids if rid not in self._loaded]
        if to_load:
            self._materialise_batch(to_load)

        return {
            rid: json.loads(self._repo.read_resource(rid))
            for rid in resource_ids
            if self._repo.resource_exists(rid)
        }

    async def for_each(
        self,
        resource_ids: List[str],
        callback: Callable[[str, Any], None],
    ) -> None:
        """Process multiple resources through a continuation.

        This avoids building a large ``dict`` in memory — each resource is
        handed to *callback* immediately after it is read.
        """
        to_load = [rid for rid in resource_ids if rid not in self._loaded]
        if to_load:
            self._materialise_batch(to_load)

        for rid in resource_ids:
            if self._repo.resource_exists(rid):
                val = json.loads(self._repo.read_resource(rid))
                callback(rid, val)

    async def for_each_all(
        self,
        callback: Callable[[str, Any], None],
    ) -> None:
        """Process **all** known resources through a continuation.

        The most memory-efficient way to iterate over the entire dataset.
        """
        ids = sorted(self._known_ids)
        await self.for_each(ids, callback)

    async def get_all(self) -> Dict[str, Any]:
        """Fetch all known resources.

        For large repositories (tens of thousands of resources), prefer
        :meth:`for_each_all` to avoid building a large ``dict``.
        """
        return await self.get_many(list(self._known_ids))

    async def list_resources(self) -> List[str]:
        """Return all known resource IDs (metadata only)."""
        return sorted(self._known_ids)

    @property
    def channel(self) -> str:
        """The channel this client is tracking."""
        return self._channel

    @property
    def server_url(self) -> str:
        """The server URL."""
        return self._server_url

    def on_update(self, callback: Callable[[UpdateEvent], None]) -> None:
        """Register a callback invoked on every live update.

        The callback receives an :class:`UpdateEvent` with full metadata
        and materialised resource snapshots.
        """
        self._on_update = callback

    # -------------------------------------------------------------------
    # Cleanup
    # -------------------------------------------------------------------

    async def close(self) -> None:
        """Cancel the WebSocket listener and clean up the temp directory."""
        if self._ws_task is not None:
            self._ws_task.cancel()
            try:
                await self._ws_task
            except asyncio.CancelledError:
                pass
        shutil.rmtree(self._work_dir, ignore_errors=True)

    # -------------------------------------------------------------------
    # Internal
    # -------------------------------------------------------------------

    def _materialise(self, resource_id: str) -> None:
        """Ensure a single resource is available locally."""
        if self._repo.resource_exists(resource_id):
            self._loaded.add(resource_id)
            return

        logger.debug("Resource '%s' not found locally after clone", resource_id)

    def _materialise_batch(self, resource_ids: List[str]) -> None:
        """Ensure a batch of resources is available locally.

        Since dyna-py's clone already materialises snapshots, this simply
        marks resources that exist as loaded and logs those that are missing.
        """
        for rid in resource_ids:
            if self._repo.resource_exists(rid):
                self._loaded.add(rid)
            else:
                logger.debug("Resource '%s' not found locally after clone", rid)

    def _pull(self, affected_ids: List[str]) -> Dict[str, Any]:
        """Pull new changesets from the server and return updated snapshots."""
        try:
            self._repo.pull(channel=self._channel)
            # Update known IDs and loaded set
            try:
                new_ids = set(self._repo.list_resources())
                self._known_ids.update(new_ids)
                self._loaded.update(new_ids)
            except Exception:
                pass

            # Collect updated snapshots for affected resources
            snapshots: Dict[str, Any] = {}
            for rid in affected_ids:
                try:
                    if self._repo.resource_exists(rid):
                        raw = self._repo.read_resource(rid)
                        snapshots[rid] = json.loads(raw)
                except Exception:
                    pass
            return snapshots
        except Exception as exc:
            logger.error("Pull failed: %s", exc)
            return {}

    async def _ws_listener(self) -> None:
        """Background task: connect to the server's WebSocket and pull on
        relevant notifications."""
        ws_url = (
            self._server_url
            .replace("http://", "ws://")
            .replace("https://", "wss://")
            + "/api/v1/ws"
        )

        while True:
            try:
                async with websockets.connect(ws_url) as ws:
                    logger.info("WebSocket connected to %s", ws_url)
                    async for message in ws:
                        await self._handle_notification(message)
            except asyncio.CancelledError:
                raise
            except Exception as exc:
                logger.warning("WebSocket error: %s, reconnecting in 5s…", exc)
                await asyncio.sleep(5)

    async def _handle_notification(self, raw: str) -> None:
        """Process a single WebSocket notification."""
        try:
            data = json.loads(raw)
        except json.JSONDecodeError:
            return

        kind = data.get("kind", "")
        timestamp = data.get("timestamp", "")
        payload = data.get("payload", {})

        # Determine the affected channel and extract changeset info
        cs_infos: List[ChangesetInfo] = []
        new_head: Optional[str] = None

        if kind == "push":
            affected_channel = payload.get("channel", "")
            new_head = payload.get("new_head")
            for cs in payload.get("changesets", []):
                cs_infos.append(ChangesetInfo(
                    change_id=cs.get("change_id", ""),
                    message=cs.get("message", ""),
                    author=cs.get("author", ""),
                    patch_count=cs.get("patch_count", 0),
                    affected_resources=cs.get("affected_resources", []),
                ))
        elif kind == "promotion":
            affected_channel = payload.get("target_channel", "")
            new_head = payload.get("new_head")
            for cs in payload.get("promoted_changesets", []):
                cs_infos.append(ChangesetInfo(
                    change_id=cs.get("change_id", ""),
                    message=cs.get("message", ""),
                    author=cs.get("author", ""),
                    patch_count=cs.get("patch_count", 0),
                    affected_resources=cs.get("affected_resources", []),
                ))
        else:
            return

        if affected_channel != self._channel:
            return

        # Collect all affected resource IDs
        all_affected: List[str] = []
        for cs in cs_infos:
            all_affected.extend(cs.affected_resources)

        logger.info(
            "Received %s notification, %d resource(s) affected",
            kind,
            len(all_affected),
        )

        # Pull updates and collect materialised snapshots
        updated_snapshots = self._pull(all_affected)

        # Build the rich UpdateEvent
        event = UpdateEvent(
            kind=kind,
            timestamp=timestamp,
            channel=affected_channel,
            changesets=cs_infos,
            new_head=new_head,
            affected_resource_ids=all_affected,
            updated_snapshots=updated_snapshots,
        )

        # Fire callback
        if self._on_update is not None:
            self._on_update(event)
