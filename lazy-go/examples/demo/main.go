// lazy-go demo — connects to a running Dyna server, lazily loads resources,
// and prints detailed live updates (metadata + content) as they arrive.
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
	"strings"
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

	// Register a live-update callback that prints full details
	client.OnUpdate(func(event *lazycat.UpdateEvent) {
		sep := strings.Repeat("═", 72)
		thin := strings.Repeat("─", 72)

		fmt.Printf("\n%s\n", sep)
		fmt.Printf("  ⚡ Live update — %s on channel '%s'\n", event.Kind, event.Channel)
		fmt.Printf("     Timestamp : %s\n", event.Timestamp)
		if event.NewHead != nil {
			fmt.Printf("     New HEAD  : %s\n", *event.NewHead)
		}
		fmt.Printf("     Resources : %d affected\n", len(event.AffectedResourceIDs))
		fmt.Printf("%s\n", thin)

		// Print per-changeset metadata
		for i, cs := range event.Changesets {
			fmt.Printf("\n  Changeset #%d [%s]\n", i+1, cs.ChangeID)
			fmt.Printf("    Author     : %s\n", cs.Author)
			fmt.Printf("    Message    : %s\n", cs.Message)
			fmt.Printf("    Patches    : %d\n", cs.PatchCount)
			fmt.Printf("    Resources  : %s\n", strings.Join(cs.AffectedResources, ", "))
		}

		// Print updated resource snapshots
		if len(event.UpdatedSnapshots) > 0 {
			fmt.Printf("\n%s\n", thin)
			fmt.Println("  Updated resource snapshots:\n")

			for rid, snap := range event.UpdatedSnapshots {
				fmt.Printf("  📄 %s:\n", rid)
				var pretty json.RawMessage
				if err := json.Unmarshal(snap, &pretty); err == nil {
					out, _ := json.MarshalIndent(pretty, "     ", "  ")
					fmt.Printf("     %s\n\n", string(out))
				} else {
					fmt.Printf("     %s\n\n", string(snap))
				}
			}
		} else {
			fmt.Println("\n  (no snapshots available)")
		}

		fmt.Printf("%s\n", sep)
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
			fmt.Printf("  %s: %s\n", id, prettyOneLine(val))
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

func prettyOneLine(raw json.RawMessage) string {
	var v interface{}
	if err := json.Unmarshal(raw, &v); err != nil {
		return string(raw)
	}
	out, err := json.Marshal(v)
	if err != nil {
		return string(raw)
	}
	s := string(out)
	if len(s) > 80 {
		s = s[:80] + "…"
	}
	return s
}
