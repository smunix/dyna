package dynago

import (
	"encoding/json"
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
