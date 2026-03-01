//! Integration test: conflict resolution during local promote.
//!
//! Scenario:
//! 1. Create a base resource on a "setup" channel, commit it.
//! 2. Fork two channels ("alice" and "bob") from "setup".
//! 3. Alice modifies `status` to "approved", commits.
//! 4. Bob modifies `status` to "rejected" and adds a `note`, commits.
//! 5. Promote "setup" to "main" — succeeds (first merge, no conflict).
//! 6. Promote "alice" to "main" — succeeds (only alice changed vs main).
//! 7. Promote "bob" to "main" — detects conflict on `status` field.
//! 8. Resolve the conflict by keeping alice's `status` + bob's `note`.
//! 9. Verify final state on main.

use anyhow::Result;
use dyna_core::channel::promote_changesets;
use dyna_core::diff::{self, three_way_merge_checked};
use dyna_core::models::*;
use dyna_core::patch;
use itertools::Itertools;
use serde_json::{json, Value};
use vfs::{MemoryFS, VfsPath};

use dyna_cli::repository::Repository;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Create a MemoryFS-backed Repository and initialise it.
fn init_memory_repo() -> Result<Repository> {
    let vfs_root: VfsPath = MemoryFS::new().into();
    let repo = Repository::from_vfs(vfs_root);

    // Manually replicate the init logic for MemoryFS
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

    // HEAD → main
    write_vfs(&dyna.join("HEAD")?, "main")?;
    write_vfs(&dyna.join("WORKING_CHANGE")?, "")?;

    // Default config
    let config = RepoConfig::default();
    let config_str = toml::to_string_pretty(&config)?;
    write_vfs(&dyna.join("config.toml")?, &config_str)?;

    // Default main channel
    let main_channel = Channel::new("main");
    let main_json = serde_json::to_string_pretty(&main_channel)?;
    write_vfs(
        &dyna.join("channels")?.join("main.json")?,
        &main_json,
    )?;

    // Default sync state
    let sync_state = SyncState::default();
    let sync_json = serde_json::to_string_pretty(&sync_state)?;
    write_vfs(&dyna.join("sync_state.json")?, &sync_json)?;

    Ok(repo)
}

/// Write a string to a VfsPath using dyna_core compression.
fn write_vfs(path: &VfsPath, content: &str) -> Result<()> {
    use std::io::Write;
    let compressed = dyna_core::compression::compress_str(content)?;
    let mut writer = path.create_file()?;
    writer.write_all(&compressed)?;
    Ok(())
}

/// Stage a change, build a patch, create a changeset, store it, update the
/// channel, and update the snapshot. Returns the changeset.
fn commit_resource(
    repo: &Repository,
    resource_id: &str,
    previous: Option<&Value>,
    current: &Value,
    message: &str,
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
    let cs = Changeset::new("test-user".to_string(), message.to_string(), parents, patches);

    repo.store_changeset(&cs)?;
    repo.save_snapshot(resource_id, current)?;

    // Update channel
    let mut channel = repo.load_channel(&channel_name)?;
    channel.append_changeset(cs.change_id.clone());
    repo.save_channel(&channel)?;
    repo.set_working_change(Some(&cs.change_id))?;

    Ok(cs)
}

/// Perform a local promote: copy changesets from source to target channel,
/// applying three-way merge for each resource. Returns Ok(promoted_ids) or
/// Err with conflicts.
fn promote_local(
    repo: &Repository,
    source_name: &str,
    target_name: &str,
) -> Result<Vec<String>, Vec<(String, Vec<Conflict>)>> {
    let source = repo.load_channel(source_name).unwrap();
    let mut target = repo.load_channel(target_name).unwrap();

    let promoted = match promote_changesets(&source, &mut target) {
        Ok(ids) => ids,
        Err(e) => {
            return Err(vec![(
                "channel".to_string(),
                vec![Conflict {
                    resource_id: "channel".to_string(),
                    json_path: "/".to_string(),
                    base_value: Some(Value::Null),
                    local_value: Value::Null,
                    remote_value: Value::String(e.to_string()),
                }],
            )]);
        }
    };

    // For each promoted changeset, check for conflicts via three-way merge
    let mut all_conflicts: Vec<(String, Vec<Conflict>)> = Vec::new();

    for cs_id in &promoted {
        let cs = repo.load_changeset(cs_id).unwrap();
        for p in &cs.patches {
            let resource_id = &p.target_resource;

            // Base: the parent snapshot from the patch (what the patch was based on)
            let base = p
                .parent_snapshot
                .clone()
                .unwrap_or(Value::Null);

            // Local: the current snapshot on the target channel
            let local = repo
                .load_snapshot_for_channel(target_name, resource_id)
                .unwrap_or(None)
                .unwrap_or(Value::Null);

            // Remote: the result snapshot from the patch
            let remote = p
                .result_snapshot
                .clone()
                .unwrap_or(Value::Null);

            // Fast path: if base and local are both null, just take remote
            if base.is_null() && local.is_null() {
                repo.save_snapshot_for_channel(target_name, resource_id, &remote)
                    .unwrap();
                continue;
            }

            // Fast path: if local == base, no conflict — take remote
            if local == base {
                repo.save_snapshot_for_channel(target_name, resource_id, &remote)
                    .unwrap();
                continue;
            }

            // Three-way merge
            match three_way_merge_checked(&base, &local, &remote) {
                Ok(merged) => {
                    repo.save_snapshot_for_channel(target_name, resource_id, &merged)
                        .unwrap();
                }
                Err(conflicts) => {
                    all_conflicts.push((resource_id.clone(), conflicts));
                }
            }
        }
    }

    if !all_conflicts.is_empty() {
        // Save conflicts for resolution
        for (resource_id, conflicts) in &all_conflicts {
            repo.save_conflicts(resource_id, conflicts).unwrap();
        }
        return Err(all_conflicts);
    }

    // Update target channel head
    target.head_change_id = source.head_change_id.clone();
    repo.save_channel(&target).unwrap();

    Ok(promoted)
}

// ---------------------------------------------------------------------------
// Test
// ---------------------------------------------------------------------------

#[test]
fn test_conflict_resolution_on_promote() -> Result<()> {
    let repo = init_memory_repo()?;

    // ── Step 1: Create base resource on "setup" channel ──────────────────
    repo.create_channel("setup", Some("main"))?;
    repo.set_current_channel("setup")?;

    let base_doc = json!({
        "name": "Project Alpha",
        "status": "active",
        "budget": 50000,
        "team": ["Alice", "Bob"]
    });

    let base_cs = commit_resource(
        &repo,
        "projects.alpha",
        None,
        &base_doc,
        "Initial project setup",
    )?;
    println!("Base changeset: {}", &base_cs.change_id[..16]);

    // ── Step 2: Promote setup → main (first merge, no conflict) ──────────
    let promoted = promote_local(&repo, "setup", "main");
    assert!(promoted.is_ok(), "Setup → main should succeed");
    println!(
        "Setup promoted to main: {} changeset(s)",
        promoted.unwrap().len()
    );

    // ── Step 3: Fork alice and bob from setup ────────────────────────────
    repo.create_channel("alice", Some("setup"))?;
    repo.create_channel("bob", Some("setup"))?;

    // ── Step 4: Alice modifies status → "approved" and budget → 75000 ───
    repo.set_current_channel("alice")?;
    let alice_doc = json!({
        "name": "Project Alpha",
        "status": "approved",
        "budget": 75000,
        "team": ["Alice", "Bob"]
    });
    let alice_cs = commit_resource(
        &repo,
        "projects.alpha",
        Some(&base_doc),
        &alice_doc,
        "Approve project and increase budget",
    )?;
    println!("Alice changeset: {}", &alice_cs.change_id[..16]);

    // ── Step 5: Bob modifies status → "rejected" and adds note ──────────
    repo.set_current_channel("bob")?;
    let bob_doc = json!({
        "name": "Project Alpha",
        "status": "rejected",
        "budget": 50000,
        "team": ["Alice", "Bob"],
        "note": "Needs more review"
    });
    let bob_cs = commit_resource(
        &repo,
        "projects.alpha",
        Some(&base_doc),
        &bob_doc,
        "Reject project with note",
    )?;
    println!("Bob changeset: {}", &bob_cs.change_id[..16]);

    // ── Step 6: Promote alice → main (should succeed, no conflict) ──────
    let promoted = promote_local(&repo, "alice", "main");
    assert!(promoted.is_ok(), "Alice → main should succeed (no conflict)");
    println!("Alice promoted to main successfully");

    // Verify main snapshot after alice's promote
    let main_snap = repo
        .load_snapshot_for_channel("main", "projects.alpha")?
        .expect("main should have projects.alpha snapshot");
    assert_eq!(main_snap["status"], "approved");
    assert_eq!(main_snap["budget"], 75000);
    println!(
        "Main after Alice: status={}, budget={}",
        main_snap["status"], main_snap["budget"]
    );

    // ── Step 7: Promote bob → main (should detect conflict on status) ───
    let promoted = promote_local(&repo, "bob", "main");
    assert!(promoted.is_err(), "Bob → main should detect conflict");

    let conflicts = promoted.unwrap_err();
    assert_eq!(conflicts.len(), 1, "Should have 1 conflicted resource");
    let (resource_id, resource_conflicts) = &conflicts[0];
    assert_eq!(resource_id, "projects.alpha");

    // Find the status conflict
    let status_conflict = resource_conflicts
        .iter()
        .find(|c| c.json_path == "/status")
        .expect("Should have conflict on /status");
    assert_eq!(status_conflict.base_value, Some(json!("active")));
    assert_eq!(status_conflict.local_value, json!("approved"));
    assert_eq!(status_conflict.remote_value, json!("rejected"));

    println!(
        "Conflict detected on {}: path={}, base={:?}, local={}, remote={}",
        resource_id,
        status_conflict.json_path,
        status_conflict.base_value,
        status_conflict.local_value,
        status_conflict.remote_value
    );

    // ── Step 8: Resolve the conflict ─────────────────────────────────────
    // Decision: keep alice's "approved" status, but accept bob's "note" field.
    // The three-way merge already auto-merged the non-conflicting fields:
    // - budget: alice changed to 75000, bob kept 50000 → alice wins (75000)
    // - note: only bob added it → auto-merged
    // - status: conflict → we manually resolve to "approved"

    let resolved_doc = json!({
        "name": "Project Alpha",
        "status": "approved",
        "budget": 75000,
        "team": ["Alice", "Bob"],
        "note": "Needs more review"
    });

    // Save the resolved snapshot on main
    repo.save_snapshot_for_channel("main", "projects.alpha", &resolved_doc)?;

    // Clear the conflicts
    repo.clear_conflicts("projects.alpha")?;

    // Complete the promote by updating the main channel
    let mut main_channel = repo.load_channel("main")?;
    let bob_channel = repo.load_channel("bob")?;
    // Add bob's changesets that aren't already in main
    let main_set: std::collections::HashSet<String> =
        main_channel.changesets.iter().cloned().collect();
    let to_add: Vec<String> = bob_channel
        .changesets
        .iter()
        .filter(|cs_id| !main_set.contains(*cs_id))
        .cloned()
        .collect();
    for cs_id in to_add {
        main_channel.append_changeset(cs_id);
    }
    main_channel.head_change_id = bob_channel.head_change_id.clone();
    repo.save_channel(&main_channel)?;

    println!("Conflict resolved: kept 'approved' status with Bob's note");

    // ── Step 9: Verify final state ───────────────────────────────────────
    let final_snap = repo
        .load_snapshot_for_channel("main", "projects.alpha")?
        .expect("main should have final snapshot");

    assert_eq!(final_snap["name"], "Project Alpha");
    assert_eq!(final_snap["status"], "approved");
    assert_eq!(final_snap["budget"], 75000);
    assert_eq!(final_snap["note"], "Needs more review");
    assert_eq!(final_snap["team"], json!(["Alice", "Bob"]));

    println!(
        "Final state: status={}, budget={}, note={}",
        final_snap["status"], final_snap["budget"], final_snap["note"]
    );

    // Verify main channel has all 3 changesets
    let main_channel = repo.load_channel("main")?;
    assert_eq!(
        main_channel.changesets.len(),
        3,
        "Main should have 3 changesets (base + alice + bob)"
    );
    println!(
        "Main channel has {} changesets: {:?}",
        main_channel.changesets.len(),
        main_channel
            .changesets
            .iter()
            .map(|id| &id[..16])
            .collect_vec()
    );

    // Verify no remaining conflicts
    let conflicted = repo.list_conflicted_resources()?;
    assert!(
        conflicted.is_empty(),
        "No conflicts should remain after resolution"
    );

    // Verify all changesets are immutable (verify hash)
    for cs_id in &main_channel.changesets {
        let cs = repo.load_changeset(cs_id)?;
        assert!(cs.verify(), "Changeset {} should verify", &cs_id[..16]);
    }
    println!("All promoted changesets verified as immutable");

    Ok(())
}
