# lazy-py

Lazy, on-demand resource loader for Dyna servers — Python edition.

`LazyClient` uses `dyna-py` (PyO3 bindings to `dyna-cli`) as a local caching layer and a WebSocket connection for live updates.

## Quick start

```python
import asyncio
from lazy_py import LazyClient

async def main():
    client = await LazyClient.connect("http://localhost:8080", "main")

    # Fetch a single resource on demand
    value = await client.get("acme.entity.User")
    print(value)

    # List all available resource IDs
    ids = await client.list_resources()
    print(ids)

    # Register a live-update callback
    client.on_update(lambda affected: print(f"Updated: {affected}"))

    await client.close()

asyncio.run(main())
```

## Installation

```bash
pip install dyna-py websockets
pip install -e .
```

## Running the demo

```bash
# Start a dyna-server first, then:
python examples/demo.py http://localhost:8080 main
```
