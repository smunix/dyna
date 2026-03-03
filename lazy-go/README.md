# lazy-go

Lazy, on-demand resource loader for [Dyna](https://github.com/smunix/dyna) servers — Go edition.

`LazyClient` uses `dyna-go` as a local caching layer and a WebSocket connection for live updates.

## Quick start

```go
package main

import (
    "context"
    "fmt"
    "lazy-go/lazycat"
)

func main() {
    ctx := context.Background()
    client, err := lazycat.Connect(ctx, "http://localhost:8080", "main")
    if err != nil {
        panic(err)
    }
    defer client.Close()

    // Fetch a single resource on demand
    data, _ := client.Get("acme.entity.User")
    fmt.Println(string(data))

    // List all available resource IDs
    ids := client.ListResources()
    fmt.Println(ids)

    // Register a live-update callback
    client.OnUpdate(func(affected []string) {
        fmt.Printf("Updated: %v\n", affected)
    })
}
```

## Running the demo

```bash
# Start a dyna-server first, then:
go run ./examples/demo http://localhost:8080 main
```
