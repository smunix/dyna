//! Integration test: concurrent editions with conflict resolution via push/pull.
//!
//! Scenario — two users share a single "remote" repository:
//!
//! 1. Initialise a shared "remote" repo (MemoryFS) and two local repos
//!    ("alice-local" and "bob-local") that treat the remote as their upstream.
//! 2. Create a base resource on the remote and "pull" it into both locals.
//! 3. Alice edits `status` → "approved" and `budget` → 80000, commits locally.
//! 4. Bob edits `status` → "rejected" and adds `note`, commits locally.
//! 5. Alice pushes first — succeeds (remote was unchanged).
//! 6. Bob pushes — rejected because the remote head has moved.
//! 7. Bob pulls Alice's changeset, detects a conflict on `/status`.
//! 8. Bob resolves the conflict (keeps "approved" + his `note`), commits a
//!    merge changeset, and pushes again — succeeds.
//! 9. Verify the final remote state is consistent.
//!
//! Because we cannot spin up a real HTTP server in a unit test, we simulate
//! push/pull by copying changesets and channel metadata between MemoryFS-backed
//! repositories.

use anyhow::Result;
use dyna_core::channel::promote_changesets;
use dyna_core::diff::{self, three_way_merge_checked};
use dyna_core::models::*;
use dyna_core::patch;
use serde_json::{json, Value};
use std::collections::HashSet;
use vfs::{MemoryFS, VfsPath};

use dyna_cli::repository::Repository;

// ---------------------------------------------------------------------------
// Helpers (shared with the existing test, duplicated here for isolation)
// ---------------------------------------------------------------------------

/// Create a MemoryFS-backed Repository and initialise it.
fn init_memory_repo() -> Result<Repository> {
    let vfs_root: VfsPath = MemoryFS::new().into();
    let repo = Repository::from_vfs(vfs_root);
    let dyna = &repo.vfs_dyna;
    dyna.create_dir_all()?;
    for sub in &[
        "patches",
        "changesets",
        "staging",
        "channels",
        "snapshots",
        "snapshots/main",
        "conflicts",
    ] {
        dyna.join(sub)?.create_dir_all()?;
    }
    write_vfs(&dyna.join("HEAD")?, "main")?;
    write_vfs(&dyna.join("WORKING_CHANGE")?, "")?;
    let config = RepoConfig::default();
    let config_str = toml::to_string_pretty(&config)?;
    write_vfs(&dyna.join("config.toml")?, &config_str)?;
    let main_channel = Channel::new("main");
    let main_json = serde_json::to_string_pretty(&main_channel)?;
    write_vfs(&dyna.join("channels")?.join("main.json")?, &main_json)?;
    let sync_state = SyncState::default();
    let sync_json = serde_json::to_string_pretty(&sync_state)?;
    write_vfs(&dyna.join("sync_state.json")?, &sync_json)?;
    Ok(repo)
}

fn write_vfs(path: &VfsPath, content: &str) -> Result<()> {
    use std::io::Write;
    let compressed = dyna_core::compression::compress_str(content)?;
    let mut writer = path.create_file()?;
    writer.write_all(&compressed)?;
    Ok(())
}

/// Commit a resource change on the current channel and return the changeset.
fn commit_resource(
    repo: &Repository,
    resource_id: &str,
    previous: Option<&Value>,
    current: &Value,
    message: &str,
    author: &str,
) -> Result<Changeset> {
    let operations = previous
        .map(|prev| diff::diff(prev, current))
        .unwrap_or_else(|| {
            vec![PatchOperation::Add {
                path: "/".to_string(),
                value: current.clone(),
            }]
        });
    let staged = StagedChange {
        resource_id: resource_id.to_string(),
        file_path: format!("{}.json", resource_id),
        previous: previous.cloned(),
        current: current.clone(),
        operations,
    };
    let channel_name = repo.current_channel_name()?;
    let channel = repo.load_channel(&channel_name)?;
    let parents = channel
        .head_change_id
        .as_ref()
        .map(|id| vec![id.clone()])
        .unwrap_or_default();
    let patches = vec![patch::build_patch(&staged)];
    let cs = Changeset::new(author.to_string(), message.to_string(), parents, patches);
    repo.store_changeset(&cs)?;
    repo.save_snapshot(resource_id, current)?;
    let mut channel = repo.load_channel(&channel_name)?;
    channel.append_changeset(cs.change_id.clone());
    repo.save_channel(&channel)?;
    repo.set_working_change(Some(&cs.change_id))?;
    Ok(cs)
}

// ---------------------------------------------------------------------------
// Simulated push: copy changesets from local → remote, update remote channel.
// Returns Ok(()) on success, Err if remote head has diverged.
// ---------------------------------------------------------------------------
fn simulate_push(
    local: &Repository,
    remote: &Repository,
    channel_name: &str,
    expected_remote_head: Option<&str>,
) -> Result<(), String> {
    let remote_channel = remote.load_channel(channel_name).unwrap();
    // Optimistic concurrency: check that the remote head matches expectation
    if remote_channel.head_change_id.as_deref() != expected_remote_head {
        return Err(format!(
            "Push rejected: remote head is {:?}, expected {:?}",
            remote_channel.head_change_id, expected_remote_head
        ));
    }
    let local_channel = local.load_channel(channel_name).unwrap();
    // Determine which changesets are new (not yet on remote)
    let remote_set: HashSet<&String> = remote_channel.changesets.iter().collect();
    let new_ids: Vec<String> = local_channel
        .changesets
        .iter()
        .filter(|id| !remote_set.contains(id))
        .cloned()
        .collect();
    // Copy changeset objects to remote
    for cs_id in &new_ids {
        let cs = local.load_changeset(cs_id).unwrap();
        remote.store_changeset(&cs).unwrap();
    }
    // Update remote channel
    let mut remote_ch = remote.load_channel(channel_name).unwrap();
    for cs_id in &new_ids {
        remote_ch.append_changeset(cs_id.clone());
    }
    remote.save_channel(&remote_ch).unwrap();
    // Copy snapshots
    for cs_id in &new_ids {
        let cs = local.load_changeset(cs_id).unwrap();
        for p in &cs.patches {
            if let Some(ref snap) = p.result_snapshot {
                remote
                    .save_snapshot_for_channel(channel_name, &p.target_resource, snap)
                    .unwrap();
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Simulated pull: copy new changesets from remote → local, three-way merge.
// Returns Ok(()) if clean, Err(conflicts) if conflicts detected.
// ---------------------------------------------------------------------------
fn simulate_pull(
    local: &Repository,
    remote: &Repository,
    channel_name: &str,
) -> Result<(), Vec<(String, Vec<Conflict>)>> {
    let remote_channel = remote.load_channel(channel_name).unwrap();
    let local_channel = local.load_channel(channel_name).unwrap();
    let local_set: HashSet<&String> = local_channel.changesets.iter().collect();
    let new_ids: Vec<String> = remote_channel
        .changesets
        .iter()
        .filter(|id| !local_set.contains(id))
        .cloned()
        .collect();
    if new_ids.is_empty() {
        return Ok(());
    }
    // Copy changeset objects to local
    for cs_id in &new_ids {
        let cs = remote.load_changeset(cs_id).unwrap();
        local.store_changeset(&cs).unwrap();
    }
    // Three-way merge for each affected resource
    let mut all_conflicts: Vec<(String, Vec<Conflict>)> = Vec::new();
    for cs_id in &new_ids {
        let cs = local.load_changeset(cs_id).unwrap();
        for p in &cs.patches {
            let resource_id = &p.target_resource;
            let base = p.parent_snapshot.clone().unwrap_or(Value::Null);
            let local_snap = local
                .load_snapshot(resource_id)
                .unwrap_or(None)
                .unwrap_or(Value::Null);
            let remote_snap = p.result_snapshot.clone().unwrap_or(Value::Null);
            if base.is_null() && local_snap.is_null() {
                local.save_snapshot(resource_id, &remote_snap).unwrap();
                continue;
            }
            if local_snap == base {
                local.save_snapshot(resource_id, &remote_snap).unwrap();
                continue;
            }
            match three_way_merge_checked(&base, &local_snap, &remote_snap) {
                Ok(merged) => {
                    local.save_snapshot(resource_id, &merged).unwrap();
                }
                Err(conflicts) => {
                    all_conflicts.push((resource_id.clone(), conflicts));
                    local.save_conflicts(resource_id, &all_conflicts.last().unwrap().1).unwrap();
                }
            }
        }
    }
    // Update local channel with the new changesets
    let mut local_ch = local.load_channel(channel_name).unwrap();
    for cs_id in &new_ids {
        local_ch.append_changeset(cs_id.clone());
    }
    local.save_channel(&local_ch).unwrap();
    if !all_conflicts.is_empty() {
        return Err(all_conflicts);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Test
// ---------------------------------------------------------------------------

#[test]
fn test_push_pull_concurrent_conflict_resolution() -> Result<()> {
    // ── Step 1: Create remote + two locals ───────────────────────────────
    let remote = init_memory_repo()?;
    let alice_local = init_memory_repo()?;
    let bob_local = init_memory_repo()?;

    // ── Step 2: Create base resource on remote, pull into both locals ────
    let base_doc = json!({
        "name": "Project Alpha",
        "status": "active",
        "budget": 50000,
        "team": ["Alice", "Bob"]
    });
    let base_cs = commit_resource(
        &remote,
        "projects.alpha",
        None,
        &base_doc,
        "Initial project setup",
        "admin",
    )?;
    println!("Remote base changeset: {}", &base_cs.change_id[..16]);

    // Simulate initial clone/pull for both locals
    let pull_result_alice = simulate_pull(&alice_local, &remote, "main");
    assert!(pull_result_alice.is_ok(), "Alice initial pull should succeed");
    let pull_result_bob = simulate_pull(&bob_local, &remote, "main");
    assert!(pull_result_bob.is_ok(), "Bob initial pull should succeed");

    // Verify both locals have the base document
    let alice_snap = alice_local
        .load_snapshot("projects.alpha")?
        .expect("Alice should have snapshot");
    assert_eq!(alice_snap, base_doc);
    let bob_snap = bob_local
        .load_snapshot("projects.alpha")?
        .expect("Bob should have snapshot");
    assert_eq!(bob_snap, base_doc);
    println!("Both locals synced with base document");

    // ── Step 3: Alice edits locally ──────────────────────────────────────
    let alice_doc = json!({
        "name": "Project Alpha",
        "status": "approved",
        "budget": 80000,
        "team": ["Alice", "Bob"]
    });
    let alice_cs = commit_resource(
        &alice_local,
        "projects.alpha",
        Some(&base_doc),
        &alice_doc,
        "Approve project and increase budget",
        "alice",
    )?;
    println!("Alice committed: {}", &alice_cs.change_id[..16]);

    // ── Step 4: Bob edits locally (concurrently) ─────────────────────────
    let bob_doc = json!({
        "name": "Project Alpha",
        "status": "rejected",
        "budget": 50000,
        "team": ["Alice", "Bob"],
        "note": "Needs more review"
    });
    let bob_cs = commit_resource(
        &bob_local,
        "projects.alpha",
        Some(&base_doc),
        &bob_doc,
        "Reject project and add note",
        "bob",
    )?;
    println!("Bob committed: {}", &bob_cs.change_id[..16]);

    // ── Step 5: Alice pushes first — should succeed ──────────────────────
    let alice_push = simulate_push(
        &alice_local,
        &remote,
        "main",
        Some(&base_cs.change_id),
    );
    assert!(alice_push.is_ok(), "Alice push should succeed");
    println!("Alice pushed successfully");

    // Verify remote now has Alice's changeset
    let remote_channel = remote.load_channel("main")?;
    assert_eq!(remote_channel.changesets.len(), 2);
    assert_eq!(
        remote_channel.head_change_id.as_ref().unwrap(),
        &alice_cs.change_id
    );

    // ── Step 6: Bob pushes — should be rejected ──────────────────────────
    let bob_push = simulate_push(
        &bob_local,
        &remote,
        "main",
        Some(&base_cs.change_id), // Bob thinks remote head is still the base
    );
    assert!(bob_push.is_err(), "Bob push should be rejected (remote head moved)");
    println!(
        "Bob push rejected as expected: {}",
        bob_push.unwrap_err()
    );

    // ── Step 7: Bob pulls — detects conflict on /status ──────────────────
    let bob_pull = simulate_pull(&bob_local, &remote, "main");
    assert!(bob_pull.is_err(), "Bob pull should detect conflicts");
    let conflicts = bob_pull.unwrap_err();
    assert_eq!(conflicts.len(), 1, "Should have 1 conflicted resource");
    let (resource_id, resource_conflicts) = &conflicts[0];
    assert_eq!(resource_id, "projects.alpha");

    let status_conflict = resource_conflicts
        .iter()
        .find(|c| c.json_path == "/status")
        .expect("Should have conflict on /status");
    assert_eq!(status_conflict.base_value, Some(json!("active")));
    // local_value is Bob's local state (which was already modified by the merge_recursive
    // to keep the local value as default for conflicts)
    // remote_value is Alice's change
    println!(
        "Conflict on /status: base={:?}, local={}, remote={}",
        status_conflict.base_value,
        status_conflict.local_value,
        status_conflict.remote_value
    );

    // ── Step 8: Bob resolves conflict and commits merge ──────────────────
    // Decision: keep Alice's "approved" status, keep Bob's "note", take Alice's budget
    let resolved_doc = json!({
        "name": "Project Alpha",
        "status": "approved",
        "budget": 80000,
        "team": ["Alice", "Bob"],
        "note": "Needs more review"
    });
    // Save the resolved snapshot
    bob_local.save_snapshot("projects.alpha", &resolved_doc)?;
    bob_local.clear_conflicts("projects.alpha")?;

    // Commit the merge resolution as a new changeset
    // The "previous" is whatever Bob had before resolution (his local state)
    let merge_cs = commit_resource(
        &bob_local,
        "projects.alpha",
        Some(&bob_doc),
        &resolved_doc,
        "Merge: keep approved status with review note",
        "bob",
    )?;
    println!("Bob committed merge resolution: {}", &merge_cs.change_id[..16]);

    // Bob pushes again — should succeed now
    let bob_push_2 = simulate_push(
        &bob_local,
        &remote,
        "main",
        Some(&alice_cs.change_id), // Remote head is now Alice's changeset
    );
    assert!(bob_push_2.is_ok(), "Bob second push should succeed");
    println!("Bob pushed merge resolution successfully");

    // ── Step 9: Verify final remote state ────────────────────────────────
    let final_remote_channel = remote.load_channel("main")?;
    // Should have: base + alice + bob_original + merge
    assert!(
        final_remote_channel.changesets.len() >= 3,
        "Remote should have at least 3 changesets (base + alice + bob-merge), got {}",
        final_remote_channel.changesets.len()
    );

    let final_snap = remote
        .load_snapshot_for_channel("main", "projects.alpha")?
        .expect("Remote should have final snapshot");
    assert_eq!(final_snap["name"], "Project Alpha");
    assert_eq!(final_snap["status"], "approved");
    assert_eq!(final_snap["budget"], 80000);
    assert_eq!(final_snap["note"], "Needs more review");
    assert_eq!(final_snap["team"], json!(["Alice", "Bob"]));
    println!("Final remote state verified: {:?}", final_snap);

    // Verify no remaining conflicts on Bob's local
    let conflicted = bob_local.list_conflicted_resources()?;
    assert!(
        conflicted.is_empty(),
        "No conflicts should remain after resolution"
    );

    // Verify all changesets are valid
    for cs_id in &final_remote_channel.changesets {
        let cs = remote.load_changeset(cs_id)?;
        assert!(cs.verify(), "Changeset {} should verify", &cs_id[..16]);
    }
    println!("All remote changesets verified");

    Ok(())
}

#[test]
fn test_push_pull_no_conflict_independent_fields() -> Result<()> {
    // When two users edit different fields, pull should auto-merge cleanly.
    let remote = init_memory_repo()?;
    let alice_local = init_memory_repo()?;
    let bob_local = init_memory_repo()?;

    let base_doc = json!({
        "title": "RFC-001",
        "author": "committee",
        "status": "draft",
        "priority": "medium"
    });
    let base_cs = commit_resource(
        &remote,
        "rfcs.001",
        None,
        &base_doc,
        "Create RFC",
        "admin",
    )?;

    // Pull into both locals
    simulate_pull(&alice_local, &remote, "main").unwrap();
    simulate_pull(&bob_local, &remote, "main").unwrap();

    // Alice changes status
    let alice_doc = json!({
        "title": "RFC-001",
        "author": "committee",
        "status": "approved",
        "priority": "medium"
    });
    let _alice_cs = commit_resource(
        &alice_local,
        "rfcs.001",
        Some(&base_doc),
        &alice_doc,
        "Approve RFC",
        "alice",
    )?;

    // Bob changes priority
    let bob_doc = json!({
        "title": "RFC-001",
        "author": "committee",
        "status": "draft",
        "priority": "high"
    });
    let _bob_cs = commit_resource(
        &bob_local,
        "rfcs.001",
        Some(&base_doc),
        &bob_doc,
        "Raise priority",
        "bob",
    )?;

    // Alice pushes first
    simulate_push(&alice_local, &remote, "main", Some(&base_cs.change_id)).unwrap();

    // Bob pulls — should auto-merge (no conflict)
    let pull_result = simulate_pull(&bob_local, &remote, "main");
    assert!(
        pull_result.is_ok(),
        "Independent field edits should auto-merge without conflict"
    );

    // Verify Bob's local snapshot has both changes merged
    let merged_snap = bob_local
        .load_snapshot("rfcs.001")?
        .expect("Bob should have merged snapshot");
    assert_eq!(merged_snap["status"], "approved"); // Alice's change
    assert_eq!(merged_snap["priority"], "high"); // Bob's change
    assert_eq!(merged_snap["title"], "RFC-001"); // Unchanged
    assert_eq!(merged_snap["author"], "committee"); // Unchanged
    println!("Auto-merge verified: status={}, priority={}", merged_snap["status"], merged_snap["priority"]);

    Ok(())
}

#[test]
fn test_push_pull_multiple_resources() -> Result<()> {
    // Concurrent edits on different resources should never conflict.
    let remote = init_memory_repo()?;
    let alice_local = init_memory_repo()?;
    let bob_local = init_memory_repo()?;

    // Create two resources on remote
    let doc_a = json!({"name": "Resource A", "value": 1});
    let doc_b = json!({"name": "Resource B", "value": 2});
    let cs_a = commit_resource(&remote, "res.a", None, &doc_a, "Create A", "admin")?;
    let _cs_b = commit_resource(&remote, "res.b", None, &doc_b, "Create B", "admin")?;

    // Pull into both locals
    simulate_pull(&alice_local, &remote, "main").unwrap();
    simulate_pull(&bob_local, &remote, "main").unwrap();

    // Alice edits resource A
    let alice_doc_a = json!({"name": "Resource A", "value": 100});
    let _alice_cs = commit_resource(
        &alice_local,
        "res.a",
        Some(&doc_a),
        &alice_doc_a,
        "Update A",
        "alice",
    )?;

    // Bob edits resource B
    let bob_doc_b = json!({"name": "Resource B", "value": 200});
    let _bob_cs = commit_resource(
        &bob_local,
        "res.b",
        Some(&doc_b),
        &bob_doc_b,
        "Update B",
        "bob",
    )?;

    // Alice pushes first
    let remote_ch = remote.load_channel("main")?;
    simulate_push(
        &alice_local,
        &remote,
        "main",
        remote_ch.head_change_id.as_deref(),
    )
    .unwrap();

    // Bob pulls — should be clean (different resources)
    let pull_result = simulate_pull(&bob_local, &remote, "main");
    assert!(
        pull_result.is_ok(),
        "Edits on different resources should not conflict"
    );

    // Verify Bob has both updates
    let snap_a = bob_local.load_snapshot("res.a")?.expect("Should have A");
    let snap_b = bob_local.load_snapshot("res.b")?.expect("Should have B");
    assert_eq!(snap_a["value"], 100); // Alice's change
    assert_eq!(snap_b["value"], 200); // Bob's change
    println!("Multi-resource merge verified: A.value={}, B.value={}", snap_a["value"], snap_b["value"]);

    Ok(())
}
