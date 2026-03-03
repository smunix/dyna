// lazy-go demo — connects to a running Dyna server, lazily loads resources,
// and prints live updates as they arrive.
//
// Usage:
//
//	go run ./examples/demo http://localhost:8080 main
package main

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"os/signal"
	"syscall"

	"lazy-go/lazycat"
)

func main() {
	serverURL := "http://localhost:8080"
	channel := "main"
	if len(os.Args) > 1 {
		serverURL = os.Args[1]
	}
	if len(os.Args) > 2 {
		channel = os.Args[2]
	}

	fmt.Printf("Connecting to %s (channel: %s)…\n", serverURL, channel)

	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()

	client, err := lazycat.Connect(ctx, serverURL, channel)
	if err != nil {
		fmt.Fprintf(os.Stderr, "Error: %v\n", err)
		os.Exit(1)
	}
	defer client.Close()

	// Register a live-update callback
	client.OnUpdate(func(affected []string) {
		fmt.Printf("\n  ⚡ Live update — %d resource(s) changed: %v\n", len(affected), affected)
	})

	// List all known resource IDs
	ids := client.ListResources()
	fmt.Printf("\nKnown resources (%d):\n", len(ids))
	limit := 20
	if len(ids) < limit {
		limit = len(ids)
	}
	for _, id := range ids[:limit] {
		fmt.Printf("  • %s\n", id)
	}
	if len(ids) > 20 {
		fmt.Printf("  … and %d more\n", len(ids)-20)
	}

	// Lazily fetch the first resource
	if len(ids) > 0 {
		first := ids[0]
		fmt.Printf("\nFetching '%s'…\n", first)
		data, err := client.Get(first)
		if err != nil {
			fmt.Printf("  Error: %v\n", err)
		} else {
			var pretty json.RawMessage
			if err := json.Unmarshal(data, &pretty); err == nil {
				out, _ := json.MarshalIndent(pretty, "", "  ")
				fmt.Println(string(out))
			}
		}
	}

	// Stream all resources through a continuation — no large map needed.
	// This is the efficient path for 59,000+ resources.
	fmt.Println("\nStreaming all resources via ForEachAll…")
	count := 0
	err = client.ForEachAll(func(id string, val json.RawMessage) error {
		count++
		if count <= 5 {
			s := string(val)
			if len(s) > 80 {
				s = s[:80] + "…"
			}
			fmt.Printf("  %s: %s\n", id, s)
		}
		return nil
	})
	if err != nil {
		fmt.Printf("  Error: %v\n", err)
	}
	fmt.Printf("  … streamed %d resource(s) total\n", count)

	// Keep alive for WebSocket updates
	fmt.Println("\nListening for live updates (Ctrl-C to quit)…")
	sig := make(chan os.Signal, 1)
	signal.Notify(sig, syscall.SIGINT, syscall.SIGTERM)
	<-sig
	fmt.Println("\nBye.")
}
