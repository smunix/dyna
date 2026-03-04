"""
lazy-py — Lazy, on-demand resource loader for Dyna servers.

Uses ``dyna-py`` (PyO3 bindings to ``dyna-cli``) as a local caching layer
and a WebSocket connection for live updates.

Quick start::

    import asyncio
    from lazy_py import LazyClient

    async def main():
        client = await LazyClient.connect("http://localhost:8080", "main")
        value = await client.get("acme.entity.User")
        print(value)

    asyncio.run(main())
"""

from lazy_py.client import ChangesetInfo, LazyClient, UpdateEvent

__all__ = ["ChangesetInfo", "LazyClient", "UpdateEvent"]
__version__ = "0.1.0"
