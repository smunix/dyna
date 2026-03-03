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
	"sort"
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
	sort.Strings(ids)
	fmt.Printf("\nKnown resources (%d):\n", len(ids))
	for _, id := range ids {
		fmt.Printf("  • %s\n", id)
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

	// Fetch all resources
	fmt.Println("\nFetching all resources…")
	all, err := client.GetAll()
	if err != nil {
		fmt.Printf("  Error: %v\n", err)
	} else {
		for id, val := range all {
			s := string(val)
			if len(s) > 80 {
				s = s[:80] + "…"
			}
			fmt.Printf("  %s: %s\n", id, s)
		}
	}

	// Keep alive for WebSocket updates
	fmt.Println("\nListening for live updates (Ctrl-C to quit)…")
	sig := make(chan os.Signal, 1)
	signal.Notify(sig, syscall.SIGINT, syscall.SIGTERM)
	<-sig
	fmt.Println("\nBye.")
}
