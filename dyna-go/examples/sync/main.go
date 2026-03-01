// Package main demonstrates dyna-go sync operations with a remote server.
//
// Prerequisites: a running dyna-server at the URL below.
//
//	dyna-server --port 8080
package main

import (
	"context"
	"encoding/json"
	"fmt"
	"log"
	"time"

	"github.com/smunix/dyna/dyna-go/dynago"
)

func main() {
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()

	serverURL := "http://localhost:8080"

	// -----------------------------------------------------------------------
	// 1. Clone from remote
	// -----------------------------------------------------------------------
	fmt.Println("=== Clone from remote ===")
	client := dynago.NewMemClient()
	if err := client.CloneRepo(ctx, serverURL); err != nil {
		log.Fatalf("Clone failed: %v", err)
	}
	fmt.Println("Cloned successfully")

	channels, _ := client.ListChannels()
	for _, ch := range channels {
		fmt.Printf("  Channel: %s\n", ch.Name)
	}

	// -----------------------------------------------------------------------
	// 2. Create a feature channel and make changes
	// -----------------------------------------------------------------------
	fmt.Println("\n=== Create feature channel ===")
	client.CreateChannel("feature/go-integration", nil)
	client.SwitchChannel("feature/go-integration")

	data, _ := json.MarshalIndent(map[string]interface{}{
		"name":    "GoService",
		"version": "1.0.0",
		"runtime": "go1.22",
	}, "", "  ")
	client.WriteResource("services.GoService", data)
	client.AddByResourceID("services.GoService")
	changeID, _ := client.Commit("Add GoService definition")
	fmt.Printf("Committed: %s\n", changeID)

	// -----------------------------------------------------------------------
	// 3. Push to remote
	// -----------------------------------------------------------------------
	fmt.Println("\n=== Push to remote ===")
	pushResp, err := client.Push(ctx)
	if err != nil {
		log.Fatalf("Push failed: %v", err)
	}
	fmt.Printf("Push success: %v\n", pushResp.Success)

	// -----------------------------------------------------------------------
	// 4. Promote to main (remote-first)
	// -----------------------------------------------------------------------
	fmt.Println("\n=== Promote to main ===")
	promoteResp, err := client.Promote(ctx, "feature/go-integration", "main")
	if err != nil {
		log.Fatalf("Promote failed: %v", err)
	}
	fmt.Printf("Promote success: %v, promoted %d changesets\n",
		promoteResp.Success, len(promoteResp.PromotedChangesets))

	// -----------------------------------------------------------------------
	// 5. Pull latest from main
	// -----------------------------------------------------------------------
	fmt.Println("\n=== Pull from main ===")
	client.SwitchChannel("main")
	pullResp, err := client.Pull(ctx)
	if err != nil {
		log.Fatalf("Pull failed: %v", err)
	}
	fmt.Printf("Pulled %d changesets\n", len(pullResp.Changesets))

	// -----------------------------------------------------------------------
	// 6. Query resource history
	// -----------------------------------------------------------------------
	fmt.Println("\n=== Resource history ===")
	history, err := client.ResourceHistory(ctx, "services.GoService")
	if err != nil {
		log.Fatalf("History failed: %v", err)
	}
	for _, entry := range history.Entries {
		fmt.Printf("  %s  %s  %s\n", entry.ChangeID[:8], entry.Author, entry.Message)
	}

	// -----------------------------------------------------------------------
	// 7. Health check
	// -----------------------------------------------------------------------
	fmt.Println("\n=== Health check ===")
	health, err := client.Health(ctx)
	if err != nil {
		log.Fatalf("Health failed: %v", err)
	}
	fmt.Printf("Server status: %s\n", health.Status)

	fmt.Println("\n=== Done ===")
}
