#!/usr/bin/env python3
"""
lazy-py demo — connects to a running Dyna server, lazily loads resources,
and prints detailed live updates (metadata + content) as they arrive.

Usage::

    lazy-py-demo [server_url] [channel]
    lazy-py-demo http://localhost:8080 main
"""

import asyncio
import json
import sys

from lazy_py import LazyClient
from lazy_py.client import UpdateEvent


async def async_main() -> None:
    server_url = sys.argv[1] if len(sys.argv) > 1 else "http://localhost:8080"
    channel = sys.argv[2] if len(sys.argv) > 2 else "main"

    print(f"Connecting to {server_url} (channel: {channel})…")
    client = await LazyClient.connect(server_url, channel)

    # Register a live-update callback that prints full details
    def on_update(event: UpdateEvent) -> None:
        sep = "═" * 72
        thin = "─" * 72

        print(f"\n{sep}")
        print(f"  ⚡ Live update — {event.kind} on channel '{event.channel}'")
        print(f"     Timestamp : {event.timestamp}")
        if event.new_head:
            print(f"     New HEAD  : {event.new_head}")
        print(f"     Resources : {len(event.affected_resource_ids)} affected")
        print(thin)

        # Print per-changeset metadata
        for i, cs in enumerate(event.changesets):
            print(f"\n  Changeset #{i + 1} [{cs.change_id}]")
            print(f"    Author     : {cs.author}")
            print(f"    Message    : {cs.message}")
            print(f"    Patches    : {cs.patch_count}")
            print(f"    Resources  : {', '.join(cs.affected_resources)}")

        # Print updated resource snapshots
        if event.updated_snapshots:
            print(f"\n{thin}")
            print("  Updated resource snapshots:\n")

            for rid, value in event.updated_snapshots.items():
                print(f"  📄 {rid}:")
                try:
                    pretty = json.dumps(value, indent=2)
                    for line in pretty.splitlines():
                        print(f"     {line}")
                except Exception as e:
                    print(f"     <serialisation error: {e}>")
                print()
        else:
            print("\n  (no snapshots available)")

        print(sep)

    client.on_update(on_update)

    # List all known resource IDs
    ids = await client.list_resources()
    print(f"\nKnown resources ({len(ids)}):")
    for rid in ids[:20]:
        print(f"  • {rid}")
    if len(ids) > 20:
        print(f"  … and {len(ids) - 20} more")

    # Lazily fetch the first resource
    if ids:
        first = ids[0]
        print(f"\nFetching '{first}'…")
        value = await client.get(first)
        print(json.dumps(value, indent=2))

    # Stream all resources through a continuation — no large dict needed.
    # This is the efficient path for 59,000+ resources.
    print("\nStreaming all resources via for_each_all…")
    count = 0

    def process(rid: str, val: object) -> None:
        nonlocal count
        count += 1
        if count <= 5:
            print(f"  {rid}: {json.dumps(val)[:80]}")

    await client.for_each_all(process)
    print(f"  … streamed {count} resource(s) total")

    # Keep alive for WebSocket updates
    print("\nListening for live updates (Ctrl-C to quit)…")
    try:
        await asyncio.Event().wait()
    except asyncio.CancelledError:
        pass
    finally:
        await client.close()


def main() -> None:
    """Synchronous entry point for console_scripts."""
    try:
        asyncio.run(async_main())
    except KeyboardInterrupt:
        print("\nBye.")


if __name__ == "__main__":
    main()
