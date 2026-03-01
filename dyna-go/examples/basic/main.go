// Package main demonstrates basic dyna-go usage with an in-memory repository.
package main

import (
	"encoding/json"
	"fmt"
	"log"

	"github.com/smunix/dyna/dyna-go/dynago"
)

func main() {
	// Create an in-memory client — no disk I/O, perfect for testing and
	// embedding in Go applications.
	client := dynago.NewMemClient()

	// Initialise the repository (no remote server needed for local ops).
	if err := client.Init(nil); err != nil {
		log.Fatal(err)
	}
	if err := client.SetUserName("alice"); err != nil {
		log.Fatal(err)
	}

	// Create a feature channel (main is protected).
	if err := client.CreateChannel("feature/user-model", nil); err != nil {
		log.Fatal(err)
	}
	if err := client.SwitchChannel("feature/user-model"); err != nil {
		log.Fatal(err)
	}

	fmt.Println("=== Write resources ===")

	// Write JSON resources using dot-separated resource IDs.
	// "acme.entity.User" maps to the file path "acme/entity/User.json".
	user := map[string]interface{}{
		"name":  "Alice",
		"email": "alice@example.com",
		"age":   30,
	}
	data, _ := json.MarshalIndent(user, "", "  ")
	if err := client.WriteResource("acme.entity.User", data); err != nil {
		log.Fatal(err)
	}

	role := map[string]interface{}{
		"name":        "admin",
		"permissions": []string{"read", "write", "delete"},
	}
	data, _ = json.MarshalIndent(role, "", "  ")
	if err := client.WriteResource("acme.entity.Role", data); err != nil {
		log.Fatal(err)
	}

	fmt.Println("=== Stage and commit ===")

	// Stage files for commit.
	if err := client.AddByResourceID("acme.entity.User"); err != nil {
		log.Fatal(err)
	}
	if err := client.AddByResourceID("acme.entity.Role"); err != nil {
		log.Fatal(err)
	}

	// Check status before committing.
	status, _ := client.Status()
	fmt.Printf("Channel: %s\n", status.Channel)
	fmt.Printf("Staged files: %d\n", len(status.Staged))
	for _, s := range status.Staged {
		fmt.Printf("  %s (%s, %d ops)\n", s.ResourceID, s.Kind, s.Ops)
	}

	// Commit.
	changeID, err := client.Commit("Add User and Role entities")
	if err != nil {
		log.Fatal(err)
	}
	fmt.Printf("Committed: %s\n", changeID)

	fmt.Println("\n=== Modify and commit again ===")

	// Update the User resource.
	user["age"] = 31
	user["role"] = "admin"
	data, _ = json.MarshalIndent(user, "", "  ")
	client.WriteResource("acme.entity.User", data)
	client.AddByResourceID("acme.entity.User")

	changeID2, _ := client.Commit("Update User age and add role")
	fmt.Printf("Committed: %s\n", changeID2)

	fmt.Println("\n=== View log ===")

	entries, _ := client.Log()
	for _, e := range entries {
		fmt.Printf("  %s  %s  %s  (%d patches)\n",
			e.ChangeID[:8], e.Author, e.Message, e.PatchCount)
	}

	fmt.Println("\n=== View diff ===")

	diff, _ := client.Diff(&changeID2)
	for _, p := range diff.Patches {
		fmt.Printf("  Resource: %s\n", p.TargetResource)
		for _, op := range p.Operations {
			fmt.Printf("    %s %s\n", op.Op, op.Path)
		}
	}

	fmt.Println("\n=== Revert last commit ===")

	revertID, err := client.Revert(changeID2)
	if err != nil {
		log.Fatal(err)
	}
	fmt.Printf("Reverted to: %s\n", revertID)

	// Verify the snapshot is back to original.
	snap, _ := client.GetSnapshot("acme.entity.User")
	fmt.Printf("User snapshot after revert: %s\n", string(snap))

	fmt.Println("\n=== Describe (amend) last commit ===")

	describedID, _ := client.Describe("Revert: undo User update")
	fmt.Printf("Described: %s\n", describedID)

	fmt.Println("\n=== Channel operations ===")

	client.CreateChannel("hotfix/urgent", nil)
	channels, _ := client.ListChannels()
	for _, ch := range channels {
		head := "(empty)"
		if ch.HeadChangeID != nil {
			head = (*ch.HeadChangeID)[:8]
		}
		fmt.Printf("  %s  head=%s  changesets=%d\n", ch.Name, head, len(ch.Changesets))
	}

	fmt.Println("\n=== Done ===")
}
