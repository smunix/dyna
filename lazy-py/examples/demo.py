#!/usr/bin/env python3
"""
lazy-py demo — connects to a running Dyna server, lazily loads resources,
and prints live updates as they arrive.

Usage::

    python demo.py [server_url] [channel]
    python demo.py http://localhost:8080 main
"""

import asyncio
import json
import sys

from lazy_py import LazyClient


async def main() -> None:
    server_url = sys.argv[1] if len(sys.argv) > 1 else "http://localhost:8080"
    channel = sys.argv[2] if len(sys.argv) > 2 else "main"

    print(f"Connecting to {server_url} (channel: {channel})…")
    client = await LazyClient.connect(server_url, channel)

    # Register a live-update callback
    def on_update(affected: list[str]) -> None:
        print(f"\n  ⚡ Live update — {len(affected)} resource(s) changed: {affected}")

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


if __name__ == "__main__":
    try:
        asyncio.run(main())
    except KeyboardInterrupt:
        print("\nBye.")
