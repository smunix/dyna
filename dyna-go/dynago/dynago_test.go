package dynago

import (
	"encoding/json"
	"strings"
	"testing"
)

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

func mustJSON(v interface{}) json.RawMessage {
	data, err := json.Marshal(v)
	if err != nil {
		panic(err)
	}
	return data
}

func newTestClient(t *testing.T) *Client {
	t.Helper()
	c := NewMemClient()
	if err := c.Init(nil); err != nil {
		t.Fatalf("Init failed: %v", err)
	}
	if err := c.SetUserName("tester"); err != nil {
		t.Fatalf("SetUserName failed: %v", err)
	}
	// Switch to a feature channel (main is protected)
	if err := c.CreateChannel("dev", nil); err != nil {
		t.Fatalf("CreateChannel failed: %v", err)
	}
	if err := c.SwitchChannel("dev"); err != nil {
		t.Fatalf("SwitchChannel failed: %v", err)
	}
	return c
}

// ---------------------------------------------------------------------------
// Models
// ---------------------------------------------------------------------------

func TestGenerateChangeID(t *testing.T) {
	id := GenerateChangeID()
	if len(id) != 16 {
		t.Errorf("expected 16 chars, got %d: %s", len(id), id)
	}
}

func TestContentHash(t *testing.T) {
	h := ContentHash([]byte("hello"))
	if h[:7] != "sha256:" {
		t.Errorf("expected sha256: prefix, got %s", h)
	}
	if len(h) != 7+64 {
		t.Errorf("expected 71 chars, got %d", len(h))
	}
}

func TestPatchVerify(t *testing.T) {
	p := NewPatch("res.1", []PatchOperation{
		{Op: OpReplace, Path: "/name", Value: mustJSON("Alice")},
	}, nil, nil, nil, nil)
	if !p.Verify() {
		t.Error("patch verification failed")
	}
}

func TestChangesetVerify(t *testing.T) {
	p := NewPatch("res.1", []PatchOperation{
		{Op: OpReplace, Path: "/name", Value: mustJSON("Alice")},
	}, nil, nil, nil, nil)
	cs := NewChangeset("alice", "test commit", nil, []Patch{p})
	if !cs.Verify() {
		t.Error("changeset verification failed")
	}
	if cs.TotalOperations() != 1 {
		t.Errorf("expected 1 op, got %d", cs.TotalOperations())
	}
	resources := cs.AffectedResources()
	if len(resources) != 1 || resources[0] != "res.1" {
		t.Errorf("unexpected affected resources: %v", resources)
	}
}

func TestChangesetRecomputeHash(t *testing.T) {
	cs := NewChangeset("alice", "draft", nil, nil)
	oldHash := cs.CommitHash
	cs.Message = "updated"
	cs.RecomputeHash()
	if cs.CommitHash == oldHash {
		t.Error("hash should have changed after message update")
	}
	if !cs.Verify() {
		t.Error("verification failed after recompute")
	}
}

func TestChannelOperations(t *testing.T) {
	ch := NewChannel("test")
	if ch.HeadChangeID != nil {
		t.Error("new channel should have nil head")
	}
	ch.AppendChangeset("aaa")
	ch.AppendChangeset("bbb")
	ch.AppendChangeset("ccc")

	if *ch.HeadChangeID != "ccc" {
		t.Errorf("expected head=ccc, got %s", *ch.HeadChangeID)
	}

	since := ch.ChangesetsSince(strPtr("aaa"))
	if len(since) != 2 || since[0] != "bbb" || since[1] != "ccc" {
		t.Errorf("unexpected since result: %v", since)
	}

	all := ch.ChangesetsSince(nil)
	if len(all) != 3 {
		t.Errorf("expected 3, got %d", len(all))
	}
}

// ---------------------------------------------------------------------------
// Diff & Patch
// ---------------------------------------------------------------------------

func TestDiffSimpleReplace(t *testing.T) {
	old := mustJSON(map[string]interface{}{"name": "Alice", "age": 30})
	new := mustJSON(map[string]interface{}{"name": "Alice", "age": 31})
	ops, err := Diff(old, new)
	if err != nil {
		t.Fatal(err)
	}
	if len(ops) != 1 {
		t.Fatalf("expected 1 op, got %d", len(ops))
	}
	if ops[0].Op != OpReplace || ops[0].Path != "/age" {
		t.Errorf("unexpected op: %+v", ops[0])
	}
}

func TestDiffAddAndRemove(t *testing.T) {
	old := mustJSON(map[string]interface{}{"name": "Alice", "age": 30})
	new := mustJSON(map[string]interface{}{"name": "Alice", "email": "alice@example.com"})
	ops, err := Diff(old, new)
	if err != nil {
		t.Fatal(err)
	}

	hasRemove := false
	hasAdd := false
	for _, op := range ops {
		if op.Op == OpRemove && op.Path == "/age" {
			hasRemove = true
		}
		if op.Op == OpAdd && op.Path == "/email" {
			hasAdd = true
		}
	}
	if !hasRemove {
		t.Error("expected Remove /age")
	}
	if !hasAdd {
		t.Error("expected Add /email")
	}
}

func TestApplyPatch(t *testing.T) {
	doc := mustJSON(map[string]interface{}{"name": "Alice", "age": 30})
	ops := []PatchOperation{
		{Op: OpReplace, Path: "/age", Value: mustJSON(31)},
		{Op: OpAdd, Path: "/email", Value: mustJSON("alice@example.com")},
	}
	if err := ApplyPatch(&doc, ops); err != nil {
		t.Fatal(err)
	}

	var result map[string]interface{}
	json.Unmarshal(doc, &result)
	if result["age"] != float64(31) {
		t.Errorf("expected age=31, got %v", result["age"])
	}
	if result["email"] != "alice@example.com" {
		t.Errorf("expected email=alice@example.com, got %v", result["email"])
	}
}

func TestThreeWayMergeNoConflict(t *testing.T) {
	base := mustJSON(map[string]interface{}{"name": "Alice", "age": 30, "status": "active"})
	local := mustJSON(map[string]interface{}{"name": "Alice", "age": 31, "status": "active"})
	remote := mustJSON(map[string]interface{}{"name": "Alice", "age": 30, "status": "inactive"})

	merged, conflicts, err := ThreeWayMerge(base, local, remote)
	if err != nil {
		t.Fatal(err)
	}
	if len(conflicts) != 0 {
		t.Errorf("expected no conflicts, got %d", len(conflicts))
	}

	var result map[string]interface{}
	json.Unmarshal(merged, &result)
	if result["age"] != float64(31) {
		t.Errorf("expected age=31, got %v", result["age"])
	}
	if result["status"] != "inactive" {
		t.Errorf("expected status=inactive, got %v", result["status"])
	}
}

func TestThreeWayMergeConflict(t *testing.T) {
	base := mustJSON(map[string]interface{}{"status": "active"})
	local := mustJSON(map[string]interface{}{"status": "approved"})
	remote := mustJSON(map[string]interface{}{"status": "rejected"})

	_, conflicts, err := ThreeWayMerge(base, local, remote)
	if err != nil {
		t.Fatal(err)
	}
	if len(conflicts) != 1 {
		t.Fatalf("expected 1 conflict, got %d", len(conflicts))
	}
	if conflicts[0].JSONPath != "/status" {
		t.Errorf("expected conflict at /status, got %s", conflicts[0].JSONPath)
	}
}

// ---------------------------------------------------------------------------
// Repository
// ---------------------------------------------------------------------------

func TestRepositoryInit(t *testing.T) {
	c := NewMemClient()
	if c.IsInitialized() {
		t.Error("should not be initialized yet")
	}
	if err := c.Init(nil); err != nil {
		t.Fatal(err)
	}
	if !c.IsInitialized() {
		t.Error("should be initialized")
	}

	// Double init should fail
	c2 := NewMemClient()
	c2.Repo = c.Repo // share the same fs
	if err := c2.Init(nil); err == nil {
		t.Error("expected error on double init")
	}
}

func TestResourceIDConversion(t *testing.T) {
	id := ResourceIDFromPath("acme/entity/User.json")
	if id != "acme.entity.User" {
		t.Errorf("expected acme.entity.User, got %s", id)
	}
	path := PathForResourceID("acme.entity.User")
	if path != "acme/entity/User.json" {
		t.Errorf("expected acme/entity/User.json, got %s", path)
	}
}

// ---------------------------------------------------------------------------
// Client operations
// ---------------------------------------------------------------------------

func TestCommitAndLog(t *testing.T) {
	c := newTestClient(t)

	// Write and stage a resource
	data := mustJSON(map[string]interface{}{"name": "Alice", "age": 30})
	if err := c.WriteResource("users.alice", data); err != nil {
		t.Fatal(err)
	}
	if err := c.AddByResourceID("users.alice"); err != nil {
		t.Fatal(err)
	}

	// Commit
	changeID, err := c.Commit("Add Alice")
	if err != nil {
		t.Fatal(err)
	}
	if len(changeID) != 16 {
		t.Errorf("expected 16-char change_id, got %d", len(changeID))
	}

	// Log
	entries, err := c.Log()
	if err != nil {
		t.Fatal(err)
	}
	if len(entries) != 1 {
		t.Fatalf("expected 1 log entry, got %d", len(entries))
	}
	if entries[0].Message != "Add Alice" {
		t.Errorf("expected message 'Add Alice', got '%s'", entries[0].Message)
	}
}

func TestStatus(t *testing.T) {
	c := newTestClient(t)

	status, err := c.Status()
	if err != nil {
		t.Fatal(err)
	}
	if status.Channel != "dev" {
		t.Errorf("expected channel=dev, got %s", status.Channel)
	}
	if len(status.Staged) != 0 {
		t.Errorf("expected 0 staged, got %d", len(status.Staged))
	}
}

func TestMainChannelProtection(t *testing.T) {
	c := NewMemClient()
	if err := c.Init(nil); err != nil {
		t.Fatal(err)
	}

	// Try to commit on main
	data := mustJSON(map[string]interface{}{"key": "value"})
	c.WriteResource("test.res", data)
	c.AddByResourceID("test.res")

	_, err := c.Commit("should fail")
	if err == nil {
		t.Error("expected error when committing to main")
	}
}

func TestDescribe(t *testing.T) {
	c := newTestClient(t)

	data := mustJSON(map[string]interface{}{"key": "value"})
	c.WriteResource("test.res", data)
	c.AddByResourceID("test.res")
	c.Commit("original message")

	changeID, err := c.Describe("updated message")
	if err != nil {
		t.Fatal(err)
	}

	cs, err := c.GetChangeset(changeID)
	if err != nil {
		t.Fatal(err)
	}
	if cs.Message != "updated message" {
		t.Errorf("expected 'updated message', got '%s'", cs.Message)
	}
}

func TestRevert(t *testing.T) {
	c := newTestClient(t)

	// Create initial state
	data := mustJSON(map[string]interface{}{"name": "Alice", "age": 30})
	c.WriteResource("users.alice", data)
	c.AddByResourceID("users.alice")
	c.Commit("Add Alice")

	// Modify
	data2 := mustJSON(map[string]interface{}{"name": "Alice", "age": 31})
	c.WriteResource("users.alice", data2)
	c.AddByResourceID("users.alice")
	changeID, _ := c.Commit("Update age")

	// Revert
	revertID, err := c.Revert(changeID)
	if err != nil {
		t.Fatal(err)
	}
	if len(revertID) != 16 {
		t.Errorf("expected 16-char revert change_id, got %d", len(revertID))
	}

	// Check log has 3 entries
	entries, _ := c.Log()
	if len(entries) != 3 {
		t.Errorf("expected 3 log entries, got %d", len(entries))
	}
}

func TestCherryPick(t *testing.T) {
	c := newTestClient(t)

	// Create a resource on dev
	data := mustJSON(map[string]interface{}{"name": "Alice"})
	c.WriteResource("users.alice", data)
	c.AddByResourceID("users.alice")
	c.Commit("Add Alice on dev")

	// Create another channel and switch to it
	c.CreateChannel("feature", strPtr("dev"))
	c.SwitchChannel("feature")

	// Add a NEW resource on feature (not modifying the shared one)
	data2 := mustJSON(map[string]interface{}{"service": "payments", "version": "2.0"})
	c.WriteResource("services.payments", data2)
	c.AddByResourceID("services.payments")
	featureChangeID, _ := c.Commit("Add payments service on feature")

	// Switch back to dev — the dev channel doesn't have services.payments
	// in its changeset history. The snapshot store has it because it's shared,
	// so we remove it to simulate the dev channel's actual state.
	_ = c.Repo.RemoveSnapshot("services.payments")
	_ = c.Repo.RemoveWorkFile(PathForResourceID("services.payments"))
	c.SwitchChannel("dev")

	// Cherry-pick the feature changeset onto dev
	pickedID, err := c.CherryPick(featureChangeID)
	if err != nil {
		t.Fatal(err)
	}
	if len(pickedID) != 16 {
		t.Errorf("expected 16-char cherry-pick change_id, got %d", len(pickedID))
	}

	// Verify the resource was applied
	snap, _ := c.GetSnapshot("services.payments")
	if snap == nil {
		t.Fatal("expected snapshot for services.payments after cherry-pick")
	}
	var result map[string]interface{}
	json.Unmarshal(snap, &result)
	if result["service"] != "payments" {
		t.Errorf("expected service=payments, got %v", result["service"])
	}
}

func TestChannelOperationsClient(t *testing.T) {
	c := newTestClient(t)

	channels, err := c.ListChannels()
	if err != nil {
		t.Fatal(err)
	}
	// Should have "main" and "dev"
	if len(channels) < 2 {
		t.Errorf("expected at least 2 channels, got %d", len(channels))
	}

	// Create and switch
	c.CreateChannel("feature-x", nil)
	c.SwitchChannel("feature-x")

	current, _ := c.CurrentChannel()
	if current != "feature-x" {
		t.Errorf("expected feature-x, got %s", current)
	}
}

func TestDiffCommand(t *testing.T) {
	c := newTestClient(t)

	// Stage something
	data := mustJSON(map[string]interface{}{"key": "value"})
	c.WriteResource("test.res", data)
	c.AddByResourceID("test.res")

	// Diff staged
	result, err := c.Diff(nil)
	if err != nil {
		t.Fatal(err)
	}
	if len(result.Patches) != 1 {
		t.Errorf("expected 1 patch, got %d", len(result.Patches))
	}
}

func TestSquash(t *testing.T) {
	c := newTestClient(t)

	// Create two commits
	data1 := mustJSON(map[string]interface{}{"name": "Alice"})
	c.WriteResource("users.alice", data1)
	c.AddByResourceID("users.alice")
	c.Commit("First commit")

	data2 := mustJSON(map[string]interface{}{"name": "Alice", "age": 30})
	c.WriteResource("users.alice", data2)
	c.AddByResourceID("users.alice")
	c.Commit("Second commit")

	// Squash
	squashedID, err := c.Squash()
	if err != nil {
		t.Fatal(err)
	}
	if len(squashedID) != 16 {
		t.Errorf("expected 16-char squashed change_id, got %d", len(squashedID))
	}

	// Log should show 1 entry
	entries, _ := c.Log()
	if len(entries) != 1 {
		t.Errorf("expected 1 log entry after squash, got %d", len(entries))
	}
}

func TestAddDelete(t *testing.T) {
	c := newTestClient(t)

	// Create and commit a resource
	data := mustJSON(map[string]interface{}{"name": "Alice"})
	c.WriteResource("users.alice", data)
	c.AddByResourceID("users.alice")
	c.Commit("Add Alice")

	// Delete it
	c.DeleteResource("users.alice")
	if err := c.AddDelete(PathForResourceID("users.alice")); err != nil {
		t.Fatal(err)
	}

	_, err := c.Commit("Delete Alice")
	if err != nil {
		t.Fatal(err)
	}

	// Snapshot should be gone
	snap, _ := c.GetSnapshot("users.alice")
	if snap != nil {
		t.Error("expected nil snapshot after delete")
	}
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

func strPtr(s string) *string { return &s }

// ---------------------------------------------------------------------------
// Conflict resolution: two channels make concurrent edits, promote to main
// ---------------------------------------------------------------------------

func TestConflictResolutionOnPromote(t *testing.T) {
	// ── Step 1: Initialize and create a base resource ──────────────────
	c := NewMemClient()
	if err := c.Init(nil); err != nil {
		t.Fatalf("Init failed: %v", err)
	}
	if err := c.SetUserName("admin"); err != nil {
		t.Fatalf("SetUserName failed: %v", err)
	}

	// Create a "setup" channel to establish the base state
	if err := c.CreateChannel("setup", nil); err != nil {
		t.Fatalf("CreateChannel(setup) failed: %v", err)
	}
	if err := c.SwitchChannel("setup"); err != nil {
		t.Fatalf("SwitchChannel(setup) failed: %v", err)
	}

	// Write the base document: a user profile with status="active"
	baseDoc := mustJSON(map[string]interface{}{
		"name":   "Project Alpha",
		"status": "active",
		"budget": 50000,
		"team":   []string{"Alice", "Bob"},
	})
	if err := c.WriteResource("projects.alpha", baseDoc); err != nil {
		t.Fatalf("WriteResource failed: %v", err)
	}
	if err := c.AddByResourceID("projects.alpha"); err != nil {
		t.Fatalf("AddByResourceID failed: %v", err)
	}
	setupChangeID, err := c.Commit("Initial project setup")
	if err != nil {
		t.Fatalf("Commit failed: %v", err)
	}
	t.Logf("Base changeset: %s", setupChangeID)

	// Promote setup to main so main has the base state
	result, err := c.Repo.PromoteLocal("setup", "main")
	if err != nil {
		t.Fatalf("PromoteLocal(setup->main) failed: %v", err)
	}
	if len(result.Conflicts) != 0 {
		for resID, conflicts := range result.Conflicts {
			for _, c := range conflicts {
				t.Logf("DEBUG conflict on %s at %s: base=%s local=%s remote=%s",
					resID, c.JSONPath, string(c.BaseValue), string(c.LocalValue), string(c.RemoteValue))
			}
		}
		t.Fatalf("Expected no conflicts promoting setup to main, got %d", len(result.Conflicts))
	}
	t.Log("Base state promoted to main successfully")

	// ── Step 2: Fork two channels from setup ───────────────────────────
	if err := c.CreateChannel("alice", strPtr("setup")); err != nil {
		t.Fatalf("CreateChannel(alice) failed: %v", err)
	}
	if err := c.CreateChannel("bob", strPtr("setup")); err != nil {
		t.Fatalf("CreateChannel(bob) failed: %v", err)
	}

	// ── Step 3: Alice changes status to "approved" and budget to 75000 ─
	if err := c.SwitchChannel("alice"); err != nil {
		t.Fatalf("SwitchChannel(alice) failed: %v", err)
	}

	aliceDoc := mustJSON(map[string]interface{}{
		"name":   "Project Alpha",
		"status": "approved",
		"budget": 75000,
		"team":   []string{"Alice", "Bob"},
	})
	if err := c.WriteResource("projects.alpha", aliceDoc); err != nil {
		t.Fatalf("WriteResource(alice) failed: %v", err)
	}
	if err := c.AddByResourceID("projects.alpha"); err != nil {
		t.Fatalf("AddByResourceID(alice) failed: %v", err)
	}
	aliceChangeID, err := c.Commit("Approve project and increase budget")
	if err != nil {
		t.Fatalf("Commit(alice) failed: %v", err)
	}
	t.Logf("Alice's changeset: %s", aliceChangeID)

	// ── Step 4: Bob changes status to "rejected" and adds a note ───────
	if err := c.SwitchChannel("bob"); err != nil {
		t.Fatalf("SwitchChannel(bob) failed: %v", err)
	}

	bobDoc := mustJSON(map[string]interface{}{
		"name":   "Project Alpha",
		"status": "rejected",
		"budget": 50000,
		"team":   []string{"Alice", "Bob"},
		"note":   "Needs more review",
	})
	if err := c.WriteResource("projects.alpha", bobDoc); err != nil {
		t.Fatalf("WriteResource(bob) failed: %v", err)
	}
	if err := c.AddByResourceID("projects.alpha"); err != nil {
		t.Fatalf("AddByResourceID(bob) failed: %v", err)
	}
	bobChangeID, err := c.Commit("Reject project with review note")
	if err != nil {
		t.Fatalf("Commit(bob) failed: %v", err)
	}
	t.Logf("Bob's changeset: %s", bobChangeID)

	// ── Step 5: Promote Alice's channel to main — should succeed ───────
	result, err = c.Repo.PromoteLocal("alice", "main")
	if err != nil {
		t.Fatalf("PromoteLocal(alice->main) failed: %v", err)
	}
	if len(result.Conflicts) != 0 {
		for resID, conflicts := range result.Conflicts {
			for _, c := range conflicts {
				t.Logf("DEBUG alice->main conflict on %s at %s: base=%s local=%s remote=%s",
					resID, c.JSONPath, string(c.BaseValue), string(c.LocalValue), string(c.RemoteValue))
			}
		}
		t.Fatalf("Expected no conflicts promoting alice to main, got %d resources with conflicts", len(result.Conflicts))
	}
	if len(result.PromotedIDs) != 1 || result.PromotedIDs[0] != aliceChangeID {
		t.Errorf("Expected promoted IDs [%s], got %v", aliceChangeID, result.PromotedIDs)
	}
	t.Log("Alice's changes promoted to main successfully (no conflict)")

	// Verify main now has Alice's version
	c.SwitchChannel("main")
	mainSnap, err := c.GetSnapshot("projects.alpha")
	if err != nil {
		t.Fatalf("GetSnapshot(main) failed: %v", err)
	}
	var mainDoc map[string]interface{}
	json.Unmarshal(mainSnap, &mainDoc)
	if mainDoc["status"] != "approved" {
		t.Errorf("Expected main status='approved', got '%v'", mainDoc["status"])
	}
	if mainDoc["budget"] != float64(75000) {
		t.Errorf("Expected main budget=75000, got %v", mainDoc["budget"])
	}
	t.Logf("Main snapshot after Alice's promote: status=%v, budget=%v", mainDoc["status"], mainDoc["budget"])

	// ── Step 6: Promote Bob's channel to main — should detect conflict ─
	//
	// Base:  status="active",   budget=50000
	// Main:  status="approved", budget=75000  (from Alice)
	// Bob:   status="rejected", budget=50000, note="Needs more review"
	//
	// Expected:
	//   - "status" conflicts (both changed differently from base)
	//   - "budget" auto-merges (only Alice changed it)
	//   - "note" auto-merges (only Bob added it)
	result, err = c.Repo.PromoteLocal("bob", "main")
	if err != nil {
		t.Fatalf("PromoteLocal(bob->main) failed: %v", err)
	}

	// There should be conflicts on the "status" field
	if len(result.Conflicts) == 0 {
		t.Fatal("Expected conflicts when promoting bob to main, got none")
	}
	alphaConflicts, ok := result.Conflicts["projects.alpha"]
	if !ok {
		t.Fatal("Expected conflicts on projects.alpha")
	}
	t.Logf("Detected %d conflict(s) on projects.alpha:", len(alphaConflicts))
	for _, c := range alphaConflicts {
		t.Logf("  Path: %s, Local: %s, Remote: %s, Base: %s",
			c.JSONPath,
			string(c.LocalValue),
			string(c.RemoteValue),
			string(c.BaseValue))
	}

	// Verify the conflict is on /status
	foundStatusConflict := false
	for _, conflict := range alphaConflicts {
		if conflict.JSONPath == "/status" {
			foundStatusConflict = true
			// Local (main) should be "approved", Remote (bob) should be "rejected"
			var localVal, remoteVal string
			json.Unmarshal(conflict.LocalValue, &localVal)
			json.Unmarshal(conflict.RemoteValue, &remoteVal)
			if localVal != "approved" {
				t.Errorf("Expected local value 'approved', got '%s'", localVal)
			}
			if remoteVal != "rejected" {
				t.Errorf("Expected remote value 'rejected', got '%s'", remoteVal)
			}
		}
	}
	if !foundStatusConflict {
		t.Error("Expected a conflict on /status path")
	}

	// ── Step 7: Resolve the conflict ───────────────────────────────────
	//
	// Decision: keep "approved" status but add Bob's note and keep the
	// higher budget. This simulates a manual merge resolution.
	resolvedDoc := mustJSON(map[string]interface{}{
		"name":   "Project Alpha",
		"status": "approved",
		"budget": 75000,
		"team":   []string{"Alice", "Bob"},
		"note":   "Needs more review",
	})

	// Switch to main to resolve
	c.SwitchChannel("main")
	if err := c.Repo.ResolveConflict("projects.alpha", resolvedDoc); err != nil {
		t.Fatalf("ResolveConflict failed: %v", err)
	}
	t.Log("Conflict resolved: kept 'approved' status with Bob's note")

	// Verify conflicts are cleared
	conflictedResources, _ := c.Repo.ListConflictedResources()
	if len(conflictedResources) != 0 {
		t.Errorf("Expected no conflicted resources after resolution, got %v", conflictedResources)
	}

	// Verify the final snapshot
	finalSnap, err := c.GetSnapshot("projects.alpha")
	if err != nil {
		t.Fatalf("GetSnapshot(final) failed: %v", err)
	}
	var finalDoc map[string]interface{}
	json.Unmarshal(finalSnap, &finalDoc)

	if finalDoc["status"] != "approved" {
		t.Errorf("Expected final status='approved', got '%v'", finalDoc["status"])
	}
	if finalDoc["budget"] != float64(75000) {
		t.Errorf("Expected final budget=75000, got %v", finalDoc["budget"])
	}
	if finalDoc["note"] != "Needs more review" {
		t.Errorf("Expected final note='Needs more review', got '%v'", finalDoc["note"])
	}
	t.Log("Final state verified: status=approved, budget=75000, note=Needs more review")

	// ── Step 8: Verify the changeset history on main ───────────────────
	mainCh, _ := c.Repo.LoadChannel("main")
	t.Logf("Main channel has %d changesets: %v", len(mainCh.Changesets), mainCh.Changesets)
	if len(mainCh.Changesets) < 3 {
		t.Errorf("Expected at least 3 changesets on main (setup + alice + bob), got %d", len(mainCh.Changesets))
	}

	// Verify all promoted changesets are marked immutable
	for _, cid := range mainCh.Changesets {
		cs, err := c.Repo.LoadChangeset(cid)
		if err != nil {
			t.Errorf("Failed to load changeset %s: %v", cid, err)
			continue
		}
		if !cs.Immutable {
			t.Errorf("Expected changeset %s to be immutable after promotion", cid)
		}
	}
	t.Log("All promoted changesets verified as immutable")
}

// ===========================================================================
// Test: Push/Pull conflict resolution via simulated remote
// ===========================================================================
// Simulates the push/pull workflow between two repositories sharing a
// "server" (a third in-memory repo). Alice and Bob each work on their own
// copy, push to the server, then pull each other's changes. When both
// modify the same field, the pull detects a conflict via three-way merge.

func TestPushPullConflictResolution(t *testing.T) {
	// ── Setup: create a "server" repo and two client repos ────────────
	server := NewMemClient()
	if err := server.Init(nil); err != nil {
		t.Fatalf("server Init failed: %v", err)
	}
	server.SetUserName("server")

	alice := NewMemClient()
	if err := alice.Init(nil); err != nil {
		t.Fatalf("alice Init failed: %v", err)
	}
	alice.SetUserName("alice")

	bob := NewMemClient()
	if err := bob.Init(nil); err != nil {
		t.Fatalf("bob Init failed: %v", err)
	}
	bob.SetUserName("bob")

	// ── Step 1: Create base document on server's "dev" channel ───────
	server.CreateChannel("dev", nil)
	server.SwitchChannel("dev")
	baseDoc := mustJSON(map[string]interface{}{"title": "RFC-42", "status": "draft", "priority": 3, "tags": []string{"backend"}})
	server.WriteResource("rfc.42", baseDoc)
	server.AddByResourceID("rfc.42")
	baseChangeID, err := server.Commit("Initial RFC-42 draft")
	if err != nil {
		t.Fatalf("server commit failed: %v", err)
	}
	t.Logf("Server base changeset: %s", baseChangeID[:12])

	// ── Step 2: Both Alice and Bob "clone" — get the base changeset ──
	baseCS, _ := server.GetChangeset(baseChangeID)
	baseCS.Immutable = true

	for _, pair := range []struct {
		client *Client
		name   string
	}{{alice, "alice"}, {bob, "bob"}} {
		c := pair.client
		c.CreateChannel("dev", nil)
		c.SwitchChannel("dev")
		c.Repo.SaveChangeset(baseCS)
		ch, _ := c.Repo.LoadChannel("dev")
		ch.AppendChangeset(baseCS.ChangeID)
		c.Repo.SaveChannel(ch)
		// Apply the base snapshot
		for _, p := range baseCS.Patches {
			if p.ResultSnapshot != nil {
				c.Repo.SaveSnapshot(p.TargetResource, p.ResultSnapshot)
				c.WriteResource(p.TargetResource, p.ResultSnapshot)
			}
		}
	}

	// ── Step 3: Alice modifies status and priority, commits ──────────
	alice.SwitchChannel("dev")
	aliceDoc := mustJSON(map[string]interface{}{"title": "RFC-42", "status": "in-review", "priority": 1, "tags": []string{"backend"}})
	alice.WriteResource("rfc.42", aliceDoc)
	alice.AddByResourceID("rfc.42")
	aliceChangeID, err := alice.Commit("Move RFC-42 to in-review, bump priority")
	if err != nil {
		t.Fatalf("alice commit failed: %v", err)
	}
	t.Logf("Alice changeset: %s", aliceChangeID[:12])

	// ── Step 4: Bob modifies status and tags, commits ────────────────
	bob.SwitchChannel("dev")
	bobDoc := mustJSON(map[string]interface{}{"title": "RFC-42", "status": "approved", "priority": 3, "tags": []string{"backend", "api"}})
	bob.WriteResource("rfc.42", bobDoc)
	bob.AddByResourceID("rfc.42")
	bobChangeID, err := bob.Commit("Approve RFC-42, add api tag")
	if err != nil {
		t.Fatalf("bob commit failed: %v", err)
	}
	t.Logf("Bob changeset: %s", bobChangeID[:12])

	// ── Step 5: Alice "pushes" to server ─────────────────────────────
	aliceCS, _ := alice.GetChangeset(aliceChangeID)
	aliceCS.Immutable = true
	server.Repo.SaveChangeset(aliceCS)
	serverCh, _ := server.Repo.LoadChannel("dev")
	serverCh.AppendChangeset(aliceCS.ChangeID)
	server.Repo.SaveChannel(serverCh)
	for _, p := range aliceCS.Patches {
		if p.ResultSnapshot != nil {
			server.Repo.SaveSnapshot(p.TargetResource, p.ResultSnapshot)
		}
	}
	t.Log("Alice pushed to server")

	// ── Step 6: Bob "pulls" from server — gets Alice's changeset ─────
	// Bob doesn't have Alice's changeset yet
	_, loadErr := bob.Repo.LoadChangeset(aliceCS.ChangeID)
	if loadErr == nil {
		t.Fatal("Bob should not have Alice's changeset before pull")
	}

	// Save Alice's changeset to Bob's repo
	bob.Repo.SaveChangeset(aliceCS)
	bobCh, _ := bob.Repo.LoadChannel("dev")
	bobCh.AppendChangeset(aliceCS.ChangeID)

	// Apply with three-way merge
	var pullConflicts []Conflict
	for _, p := range aliceCS.Patches {
		localSnap, _ := bob.GetSnapshot(p.TargetResource)
		base := p.ParentSnapshot
		remote := p.ResultSnapshot

		if base == nil || remote == nil {
			if remote != nil {
				bob.Repo.SaveSnapshot(p.TargetResource, remote)
			}
			continue
		}
		if localSnap == nil {
			bob.Repo.SaveSnapshot(p.TargetResource, remote)
			continue
		}

		merged, conflicts, err := ThreeWayMerge(base, localSnap, remote)
		if err != nil {
			t.Fatalf("ThreeWayMerge failed: %v", err)
		}
		if len(conflicts) > 0 {
			pullConflicts = append(pullConflicts, conflicts...)
			bob.Repo.SaveConflicts(p.TargetResource, conflicts)
		}
		bob.Repo.SaveSnapshot(p.TargetResource, merged)
	}
	bob.Repo.SaveChannel(bobCh)

	// ── Step 7: Verify conflict was detected ─────────────────────────
	if len(pullConflicts) == 0 {
		t.Fatal("Expected conflicts during Bob's pull, got none")
	}
	t.Logf("Pull detected %d conflict(s)", len(pullConflicts))

	foundStatusConflict := false
	for _, c := range pullConflicts {
		t.Logf("  Conflict at %s: base=%s, local=%s, remote=%s",
			c.JSONPath, string(c.BaseValue), string(c.LocalValue), string(c.RemoteValue))
		if c.JSONPath == "/status" {
			foundStatusConflict = true
			var localVal, remoteVal string
			json.Unmarshal(c.LocalValue, &localVal)
			json.Unmarshal(c.RemoteValue, &remoteVal)
			if localVal != "approved" {
				t.Errorf("Expected local status='approved', got '%s'", localVal)
			}
			if remoteVal != "in-review" {
				t.Errorf("Expected remote status='in-review', got '%s'", remoteVal)
			}
		}
	}
	if !foundStatusConflict {
		t.Error("Expected conflict on /status field")
	}

	// Verify non-conflicting fields merged correctly
	mergedSnap, _ := bob.GetSnapshot("rfc.42")
	var mergedDoc map[string]interface{}
	json.Unmarshal(mergedSnap, &mergedDoc)

	// Priority: base=3, Alice=1, Bob=3 → local==base → take remote → 1
	if mergedDoc["priority"] != float64(1) {
		t.Errorf("Expected merged priority=1 (Alice's), got %v", mergedDoc["priority"])
	}
	// Tags: base=["backend"], Alice=["backend"], Bob=["backend","api"]
	// Alice didn't change → take Bob's
	tags, ok := mergedDoc["tags"].([]interface{})
	if !ok || len(tags) != 2 {
		t.Errorf("Expected merged tags to have 2 items, got %v", mergedDoc["tags"])
	}
	t.Log("Non-conflicting fields (priority, tags) merged correctly")

	// ── Step 8: Resolve the conflict ─────────────────────────────────
	resolvedDoc := mustJSON(map[string]interface{}{"title": "RFC-42", "status": "in-review", "priority": 1, "tags": []string{"backend", "api"}})
	bob.Repo.ResolveConflict("rfc.42", resolvedDoc)

	conflicted, _ := bob.ListConflicts()
	if len(conflicted) != 0 {
		t.Errorf("Expected no conflicts after resolution, got %v", conflicted)
	}

	finalSnap, _ := bob.GetSnapshot("rfc.42")
	var finalDoc map[string]interface{}
	json.Unmarshal(finalSnap, &finalDoc)
	if finalDoc["status"] != "in-review" {
		t.Errorf("Expected resolved status='in-review', got '%v'", finalDoc["status"])
	}
	if finalDoc["priority"] != float64(1) {
		t.Errorf("Expected resolved priority=1, got %v", finalDoc["priority"])
	}
	t.Log("Conflict resolved successfully")
}

// ===========================================================================
// Test: History (log) command correctness
// ===========================================================================

func TestHistoryCorrectness(t *testing.T) {
	c := NewMemClient()
	if err := c.Init(nil); err != nil {
		t.Fatalf("Init failed: %v", err)
	}
	c.SetUserName("historian")

	c.CreateChannel("dev", nil)
	c.SwitchChannel("dev")

	// Commit 1: create a user resource
	doc1 := mustJSON(map[string]interface{}{"name": "Alice", "role": "viewer"})
	c.WriteResource("users.alice", doc1)
	c.AddByResourceID("users.alice")
	id1, _ := c.Commit("Add user Alice")

	// Commit 2: update Alice's role
	doc2 := mustJSON(map[string]interface{}{"name": "Alice", "role": "editor"})
	c.WriteResource("users.alice", doc2)
	c.AddByResourceID("users.alice")
	id2, _ := c.Commit("Promote Alice to editor")

	// Commit 3: add a second resource
	doc3 := mustJSON(map[string]interface{}{"name": "Bob", "role": "admin"})
	c.WriteResource("users.bob", doc3)
	c.AddByResourceID("users.bob")
	id3, _ := c.Commit("Add user Bob")

	// Commit 4: update both resources
	doc4a := mustJSON(map[string]interface{}{"name": "Alice", "role": "admin"})
	doc4b := mustJSON(map[string]interface{}{"name": "Bob", "role": "superadmin"})
	c.WriteResource("users.alice", doc4a)
	c.WriteResource("users.bob", doc4b)
	c.AddByResourceID("users.alice")
	c.AddByResourceID("users.bob")
	id4, _ := c.Commit("Promote both to admin")

	// ── Verify log returns all 4 entries in reverse order ────────────
	entries, err := c.Log()
	if err != nil {
		t.Fatalf("Log failed: %v", err)
	}
	if len(entries) != 4 {
		t.Fatalf("Expected 4 log entries, got %d", len(entries))
	}

	expectedIDs := []string{id4, id3, id2, id1}
	expectedMessages := []string{
		"Promote both to admin",
		"Add user Bob",
		"Promote Alice to editor",
		"Add user Alice",
	}
	for i, entry := range entries {
		if entry.ChangeID != expectedIDs[i] {
			t.Errorf("Entry %d: expected change_id=%s, got %s", i, expectedIDs[i][:12], entry.ChangeID[:12])
		}
		if entry.Message != expectedMessages[i] {
			t.Errorf("Entry %d: expected message='%s', got '%s'", i, expectedMessages[i], entry.Message)
		}
		if entry.Author != "historian" {
			t.Errorf("Entry %d: expected author='historian', got '%s'", i, entry.Author)
		}
		if entry.CommitHash == "" {
			t.Errorf("Entry %d: commit_hash should not be empty", i)
		}
		if entry.CreatedAt == "" {
			t.Errorf("Entry %d: created_at should not be empty", i)
		}
	}
	t.Log("Log entries are in correct reverse chronological order")

	// ── Verify patch counts ──────────────────────────────────────────
	if entries[0].PatchCount != 2 {
		t.Errorf("Entry 0 (Promote both): expected 2 patches, got %d", entries[0].PatchCount)
	}
	if entries[1].PatchCount != 1 {
		t.Errorf("Entry 1 (Add Bob): expected 1 patch, got %d", entries[1].PatchCount)
	}
	if entries[2].PatchCount != 1 {
		t.Errorf("Entry 2 (Promote Alice): expected 1 patch, got %d", entries[2].PatchCount)
	}
	if entries[3].PatchCount != 1 {
		t.Errorf("Entry 3 (Add Alice): expected 1 patch, got %d", entries[3].PatchCount)
	}
	t.Log("Patch counts are correct")

	// ── Verify log on a different channel shows different history ─────
	c.CreateChannel("feature", strPtr("dev"))
	c.SwitchChannel("feature")

	featureEntries, _ := c.Log()
	if len(featureEntries) != 4 {
		t.Fatalf("Feature channel: expected 4 log entries (forked), got %d", len(featureEntries))
	}

	// Add a commit on feature
	doc5 := mustJSON(map[string]interface{}{"name": "Charlie", "role": "viewer"})
	c.WriteResource("users.charlie", doc5)
	c.AddByResourceID("users.charlie")
	id5, _ := c.Commit("Add Charlie on feature")

	featureEntries, _ = c.Log()
	if len(featureEntries) != 5 {
		t.Fatalf("Feature channel: expected 5 log entries after commit, got %d", len(featureEntries))
	}
	if featureEntries[0].ChangeID != id5 {
		t.Errorf("Feature channel: newest entry should be Charlie commit")
	}

	// Dev channel should still have only 4
	c.SwitchChannel("dev")
	devEntries, _ := c.Log()
	if len(devEntries) != 4 {
		t.Errorf("Dev channel: expected 4 entries (unchanged), got %d", len(devEntries))
	}
	t.Log("Channel isolation verified: feature has 5, dev has 4")

	// ── Verify main channel is empty ─────────────────────────────────
	c.SwitchChannel("main")
	mainEntries, _ := c.Log()
	if len(mainEntries) != 0 {
		t.Errorf("Main channel: expected 0 entries, got %d", len(mainEntries))
	}
	t.Log("Main channel correctly has no entries")

	// ── Verify DiffChangeset returns correct operations ──────────────
	c.SwitchChannel("dev")
	diffResult, err := c.Diff(&id2)
	if err != nil {
		t.Fatalf("Diff failed: %v", err)
	}
	if len(diffResult.Patches) != 1 {
		t.Fatalf("Diff: expected 1 patch, got %d", len(diffResult.Patches))
	}
	if diffResult.Patches[0].TargetResource != "users.alice" {
		t.Errorf("Diff: expected target_resource='users.alice', got '%s'",
			diffResult.Patches[0].TargetResource)
	}
	foundRoleChange := false
	for _, op := range diffResult.Patches[0].Operations {
		if op.Path == "/role" && op.Op == OpReplace {
			foundRoleChange = true
		}
	}
	if !foundRoleChange {
		t.Error("Diff: expected a replace operation on /role")
	}
	t.Log("DiffChangeset correctly shows role change operation")
}

// ===========================================================================
// Test: Cherry-pick correctness
// ===========================================================================

func TestCherryPickCorrectness(t *testing.T) {
	c := NewMemClient()
	if err := c.Init(nil); err != nil {
		t.Fatalf("Init failed: %v", err)
	}
	c.SetUserName("cherry-picker")

	// ── Setup: create base state on "dev" ────────────────────────────
	c.CreateChannel("dev", nil)
	c.SwitchChannel("dev")

	baseDoc := mustJSON(map[string]interface{}{"name": "Config", "version": float64(1), "debug": false})
	c.WriteResource("app.config", baseDoc)
	c.AddByResourceID("app.config")
	baseID, _ := c.Commit("Initial config")
	t.Logf("Base changeset: %s", baseID[:12])

	// ── Create "feature-a" with two commits ──────────────────────────
	c.CreateChannel("feature-a", strPtr("dev"))
	c.SwitchChannel("feature-a")

	// Commit A1: bump version
	docA1 := mustJSON(map[string]interface{}{"name": "Config", "version": float64(2), "debug": false})
	c.WriteResource("app.config", docA1)
	c.AddByResourceID("app.config")
	idA1, _ := c.Commit("Bump version to 2")
	t.Logf("Feature-A commit 1: %s", idA1[:12])

	// Commit A2: enable debug and add a new resource
	docA2 := mustJSON(map[string]interface{}{"name": "Config", "version": float64(2), "debug": true})
	c.WriteResource("app.config", docA2)
	c.AddByResourceID("app.config")
	newResource := mustJSON(map[string]interface{}{"endpoint": "/api/v2", "timeout": float64(30)})
	c.WriteResource("app.api", newResource)
	c.AddByResourceID("app.api")
	idA2, _ := c.Commit("Enable debug, add API config")
	t.Logf("Feature-A commit 2: %s", idA2[:12])

	// ── Create "feature-b" from dev (does NOT have feature-a changes) ─
	c.CreateChannel("feature-b", strPtr("dev"))
	c.SwitchChannel("feature-b")

	// Verify feature-b has the base state
	snap, _ := c.GetSnapshot("app.config")
	var snapDoc map[string]interface{}
	json.Unmarshal(snap, &snapDoc)
	if snapDoc["version"] != float64(1) {
		t.Fatalf("Feature-b should start with version=1, got %v", snapDoc["version"])
	}
	t.Log("Feature-b starts with base state (version=1)")

	// ── Cherry-pick A1 (version bump) into feature-b ─────────────────
	cherryID, err := c.CherryPick(idA1)
	if err != nil {
		t.Fatalf("CherryPick(A1) failed: %v", err)
	}
	t.Logf("Cherry-picked A1 as: %s", cherryID[:12])

	snap, _ = c.GetSnapshot("app.config")
	json.Unmarshal(snap, &snapDoc)
	if snapDoc["version"] != float64(2) {
		t.Errorf("After cherry-pick A1: expected version=2, got %v", snapDoc["version"])
	}
	if snapDoc["debug"] != false {
		t.Errorf("After cherry-pick A1: debug should still be false")
	}
	t.Log("Cherry-pick A1 correctly bumped version to 2, debug unchanged")

	// ── Verify the cherry-picked changeset is independent ────────────
	cherryCS, _ := c.GetChangeset(cherryID)
	if cherryCS.ChangeID == idA1 {
		t.Error("Cherry-picked changeset should have a different change_id")
	}
	if len(cherryCS.Patches) != 1 {
		t.Errorf("Cherry-picked changeset should have 1 patch, got %d", len(cherryCS.Patches))
	}
	if cherryCS.Patches[0].TargetResource != "app.config" {
		t.Errorf("Cherry-picked patch should target 'app.config', got '%s'",
			cherryCS.Patches[0].TargetResource)
	}
	if !strings.Contains(cherryCS.Message, "Cherry-pick") {
		t.Errorf("Cherry-pick message should contain 'Cherry-pick', got '%s'", cherryCS.Message)
	}
	t.Log("Cherry-picked changeset is independent with correct metadata")

	// ── Cherry-pick A2 (debug + new resource) into feature-b ─────────
	cherryID2, err := c.CherryPick(idA2)
	if err != nil {
		t.Fatalf("CherryPick(A2) failed: %v", err)
	}
	t.Logf("Cherry-picked A2 as: %s", cherryID2[:12])

	snap, _ = c.GetSnapshot("app.config")
	json.Unmarshal(snap, &snapDoc)
	if snapDoc["debug"] != true {
		t.Errorf("After cherry-pick A2: expected debug=true, got %v", snapDoc["debug"])
	}

	// Verify the new resource was created
	apiSnap, err := c.GetSnapshot("app.api")
	if err != nil || apiSnap == nil {
		t.Fatal("After cherry-pick A2: app.api resource should exist")
	}
	var apiDoc map[string]interface{}
	json.Unmarshal(apiSnap, &apiDoc)
	if apiDoc["endpoint"] != "/api/v2" {
		t.Errorf("After cherry-pick A2: expected endpoint='/api/v2', got '%v'", apiDoc["endpoint"])
	}
	if apiDoc["timeout"] != float64(30) {
		t.Errorf("After cherry-pick A2: expected timeout=30, got %v", apiDoc["timeout"])
	}
	t.Log("Cherry-pick A2 correctly enabled debug and created app.api resource")

	// ── Verify feature-b log has 3 entries (base + 2 cherry-picks) ───
	entries, _ := c.Log()
	if len(entries) != 3 {
		t.Fatalf("Feature-b should have 3 log entries, got %d", len(entries))
	}
	if !strings.Contains(entries[0].Message, "Cherry-pick") {
		t.Errorf("Entry 0 should be a cherry-pick, got '%s'", entries[0].Message)
	}
	if !strings.Contains(entries[1].Message, "Cherry-pick") {
		t.Errorf("Entry 1 should be a cherry-pick, got '%s'", entries[1].Message)
	}
	if entries[2].Message != "Initial config" {
		t.Errorf("Entry 2 should be base commit, got '%s'", entries[2].Message)
	}
	t.Log("Feature-b log correctly shows base + 2 cherry-picks")

	// ── Verify feature-a is unchanged ────────────────────────────────
	c.SwitchChannel("feature-a")
	featureAEntries, _ := c.Log()
	if len(featureAEntries) != 3 {
		t.Errorf("Feature-a should still have 3 entries, got %d", len(featureAEntries))
	}
	featureASnap, _ := c.GetSnapshot("app.config")
	var featureADoc map[string]interface{}
	json.Unmarshal(featureASnap, &featureADoc)
	if featureADoc["debug"] != true {
		t.Error("Feature-a config should still have debug=true")
	}
	t.Log("Feature-a is unchanged after cherry-picks to feature-b")

	// ── Edge case: cherry-pick into a channel with diverged state ─────
	c.SwitchChannel("feature-b")
	divergedDoc := mustJSON(map[string]interface{}{"name": "Config-Custom", "version": float64(2), "debug": true, "custom": true})
	c.WriteResource("app.config", divergedDoc)
	c.AddByResourceID("app.config")
	c.Commit("Customize config on feature-b")

	// Cherry-pick A1 again — should fail as no-op since version is already 2
	_, err = c.CherryPick(idA1)
	if err != nil {
		t.Logf("Cherry-pick A1 again (expected no-op): %v", err)
	} else {
		// Verify it didn't overwrite the custom field
		snap, _ = c.GetSnapshot("app.config")
		json.Unmarshal(snap, &snapDoc)
		if _, ok := snapDoc["custom"]; !ok {
			t.Error("Cherry-pick should not have removed the 'custom' field")
		}
	}
	t.Log("Edge case: re-cherry-pick handled correctly")
}
