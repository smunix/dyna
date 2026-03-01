//! Integration test: correctness of the cherry-pick command.
//!
//! Cherry-pick takes a changeset from one channel and applies its logical
//! diff as a new changeset on the destination channel. The new changeset
//! has a fresh `change_id`, records the destination channel's head as its
//! parent, and recomputes patch hashes.
//!
//! This test verifies:
//!
//! 1. A simple cherry-pick applies the correct operations and produces the
//!    expected snapshot on the destination channel.
//! 2. The cherry-picked changeset has a different `change_id` from the source.
//! 3. The cherry-picked changeset's parent is the destination channel's head.
//! 4. Cherry-picking onto a channel that already has divergent changes
//!    correctly applies the delta (not the absolute state).
//! 5. Cherry-picking a multi-resource changeset applies all patches.
//! 6. The cherry-picked changeset verifies its hash integrity.

use anyhow::Result;
use dyna_core::diff;
use dyna_core::models::*;
use dyna_core::patch;
use serde_json::{json, Value};
use vfs::{MemoryFS, VfsPath};

use dyna_cli::repository::Repository;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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

/// Commit a changeset with multiple resource patches.
fn commit_multi_resource(
    repo: &Repository,
    changes: Vec<(&str, Option<&Value>, &Value)>,
    message: &str,
    author: &str,
) -> Result<Changeset> {
    let channel_name = repo.current_channel_name()?;
    let channel = repo.load_channel(&channel_name)?;
    let parents = channel
        .head_change_id
        .as_ref()
        .map(|id| vec![id.clone()])
        .unwrap_or_default();

    let mut patches = Vec::new();
    for (resource_id, previous, current) in &changes {
        let operations = previous
            .map(|prev| diff::diff(prev, current))
            .unwrap_or_else(|| {
                vec![PatchOperation::Add {
                    path: "/".to_string(),
                    value: (*current).clone(),
                }]
            });
        let staged = StagedChange {
            resource_id: resource_id.to_string(),
            file_path: format!("{}.json", resource_id),
            previous: previous.cloned(),
            current: (*current).clone(),
            operations,
        };
        patches.push(patch::build_patch(&staged));
    }

    let cs = Changeset::new(author.to_string(), message.to_string(), parents, patches);
    repo.store_changeset(&cs)?;

    for (resource_id, _, current) in &changes {
        repo.save_snapshot(resource_id, current)?;
    }

    let mut channel = repo.load_channel(&channel_name)?;
    channel.append_changeset(cs.change_id.clone());
    repo.save_channel(&channel)?;
    repo.set_working_change(Some(&cs.change_id))?;
    Ok(cs)
}

/// Simulate cherry-pick: apply a source changeset's logical diff onto the
/// destination channel's current state. This mirrors the logic in
/// `dyna-cli/src/commands/cherry_pick.rs`.
fn cherry_pick(
    repo: &Repository,
    source_cs: &Changeset,
    dest_channel_name: &str,
    author: &str,
) -> Result<Changeset> {
    let cherry_patches: Vec<Patch> = source_cs
        .patches
        .iter()
        .map(|src_patch| {
            let current_snapshot = repo
                .load_snapshot(&src_patch.target_resource)
                .ok()
                .flatten()
                .unwrap_or_else(|| json!({}));

            let new_snapshot = src_patch
                .result_snapshot
                .as_ref()
                .and_then(|result| {
                    src_patch.parent_snapshot.as_ref().map(|parent| {
                        let delta_ops = diff::diff(parent, result);
                        let mut dest = current_snapshot.clone();
                        diff::apply_patch(&mut dest, &delta_ops).ok();
                        dest
                    })
                })
                .unwrap_or_else(|| {
                    src_patch
                        .result_snapshot
                        .clone()
                        .unwrap_or_else(|| current_snapshot.clone())
                });

            let operations = diff::diff(&current_snapshot, &new_snapshot);
            Patch::new(
                src_patch.target_resource.clone(),
                operations,
                Some(current_snapshot),
                Some(new_snapshot),
            )
        })
        .collect();

    let dest_channel = repo.load_channel(dest_channel_name)?;
    let parents = dest_channel
        .head_change_id
        .clone()
        .map(|id| vec![id])
        .unwrap_or_default();

    let cherry_message = format!(
        "Cherry-pick \"{}\" ({})",
        source_cs.message,
        source_cs.short_change_id()
    );
    let cherry_cs = Changeset::new(author.to_string(), cherry_message, parents, cherry_patches);

    repo.store_changeset(&cherry_cs)?;

    // Update snapshots
    for p in &cherry_cs.patches {
        if let Some(ref snap) = p.result_snapshot {
            if snap.is_null() {
                repo.remove_snapshot(&p.target_resource)?;
            } else {
                repo.save_snapshot(&p.target_resource, snap)?;
            }
        }
    }

    // Append to destination channel
    let mut channel = repo.load_channel(dest_channel_name)?;
    channel.append_changeset(cherry_cs.change_id.clone());
    repo.save_channel(&channel)?;
    repo.set_working_change(Some(&cherry_cs.change_id))?;

    Ok(cherry_cs)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn test_simple_cherry_pick() -> Result<()> {
    let repo = init_memory_repo()?;

    // Create base on a "setup" channel, then fork two channels
    repo.create_channel("setup", Some("main"))?;
    repo.set_current_channel("setup")?;
    let base_doc = json!({"name": "Widget", "version": 1, "active": true});
    let _base_cs = commit_resource(&repo, "widgets.w1", None, &base_doc, "Create widget", "admin")?;

    // Fork alice and bob from setup
    repo.create_channel("alice", Some("setup"))?;
    repo.create_channel("bob", Some("setup"))?;

    // Alice makes a change
    repo.set_current_channel("alice")?;
    let alice_doc = json!({"name": "Widget", "version": 2, "active": true});
    let alice_cs = commit_resource(
        &repo,
        "widgets.w1",
        Some(&base_doc),
        &alice_doc,
        "Bump version to 2",
        "alice",
    )?;

    // Cherry-pick Alice's change onto Bob's channel
    repo.set_current_channel("bob")?;
    let cherry_cs = cherry_pick(&repo, &alice_cs, "bob", "bob")?;

    // Verify the cherry-picked changeset
    assert_ne!(
        cherry_cs.change_id, alice_cs.change_id,
        "Cherry-pick should create a new change_id"
    );
    assert!(
        cherry_cs.message.contains("Cherry-pick"),
        "Cherry-pick message should contain 'Cherry-pick'"
    );
    assert!(
        cherry_cs.message.contains("Bump version to 2"),
        "Cherry-pick message should reference original message"
    );
    assert!(cherry_cs.verify(), "Cherry-picked changeset should verify");

    // Verify the snapshot on Bob's channel
    let bob_snap = repo
        .load_snapshot("widgets.w1")?
        .expect("Bob should have snapshot");
    assert_eq!(bob_snap["version"], 2, "Version should be bumped to 2");
    assert_eq!(bob_snap["name"], "Widget");
    assert_eq!(bob_snap["active"], true);

    // Verify parent is Bob's channel head (which was the setup base)
    let bob_channel = repo.load_channel("bob")?;
    assert_eq!(
        bob_channel.head_change_id.as_ref().unwrap(),
        &cherry_cs.change_id,
        "Bob's head should point to cherry-picked changeset"
    );

    println!("Simple cherry-pick verified");
    Ok(())
}

#[test]
fn test_cherry_pick_onto_divergent_channel() -> Result<()> {
    let repo = init_memory_repo()?;

    // Create base
    repo.create_channel("setup", Some("main"))?;
    repo.set_current_channel("setup")?;
    let base_doc = json!({"name": "Config", "debug": false, "timeout": 30});
    let _base_cs = commit_resource(&repo, "config.app", None, &base_doc, "Initial config", "admin")?;

    // Fork alice and bob
    repo.create_channel("alice", Some("setup"))?;
    repo.create_channel("bob", Some("setup"))?;

    // Alice changes debug → true
    repo.set_current_channel("alice")?;
    let alice_doc = json!({"name": "Config", "debug": true, "timeout": 30});
    let alice_cs = commit_resource(
        &repo,
        "config.app",
        Some(&base_doc),
        &alice_doc,
        "Enable debug mode",
        "alice",
    )?;

    // Bob changes timeout → 60 (independent field)
    repo.set_current_channel("bob")?;
    let bob_doc = json!({"name": "Config", "debug": false, "timeout": 60});
    let _bob_cs = commit_resource(
        &repo,
        "config.app",
        Some(&base_doc),
        &bob_doc,
        "Increase timeout",
        "bob",
    )?;

    // Cherry-pick Alice's change onto Bob's channel
    // Bob's current state: debug=false, timeout=60
    // Alice's delta: debug: false → true
    // Expected result: debug=true, timeout=60
    let cherry_cs = cherry_pick(&repo, &alice_cs, "bob", "bob")?;

    let bob_snap = repo
        .load_snapshot("config.app")?
        .expect("Bob should have snapshot");
    assert_eq!(bob_snap["debug"], true, "debug should be true (from Alice's cherry-pick)");
    assert_eq!(bob_snap["timeout"], 60, "timeout should remain 60 (Bob's change preserved)");
    assert_eq!(bob_snap["name"], "Config");

    // Verify the cherry-pick's parent is Bob's previous head (not Alice's)
    assert_eq!(cherry_cs.patches.len(), 1);
    let patch = &cherry_cs.patches[0];
    assert_eq!(patch.parent_snapshot.as_ref().unwrap(), &bob_doc);
    assert_eq!(
        patch.result_snapshot.as_ref().unwrap()["debug"], true,
        "Result snapshot should have debug=true"
    );
    assert_eq!(
        patch.result_snapshot.as_ref().unwrap()["timeout"], 60,
        "Result snapshot should preserve timeout=60"
    );

    println!("Cherry-pick onto divergent channel verified: debug=true, timeout=60");
    Ok(())
}

#[test]
fn test_cherry_pick_multi_resource() -> Result<()> {
    let repo = init_memory_repo()?;

    // Create base with two resources
    repo.create_channel("setup", Some("main"))?;
    repo.set_current_channel("setup")?;
    let doc_a = json!({"type": "A", "count": 0});
    let doc_b = json!({"type": "B", "count": 0});
    let _base_cs = commit_multi_resource(
        &repo,
        vec![
            ("items.a", None, &doc_a),
            ("items.b", None, &doc_b),
        ],
        "Create items A and B",
        "admin",
    )?;

    // Fork source and dest
    repo.create_channel("source", Some("setup"))?;
    repo.create_channel("dest", Some("setup"))?;

    // Source modifies both resources in a single changeset
    repo.set_current_channel("source")?;
    let new_a = json!({"type": "A", "count": 10});
    let new_b = json!({"type": "B", "count": 20});
    let source_cs = commit_multi_resource(
        &repo,
        vec![
            ("items.a", Some(&doc_a), &new_a),
            ("items.b", Some(&doc_b), &new_b),
        ],
        "Update both items",
        "dev",
    )?;
    assert_eq!(source_cs.patches.len(), 2, "Source should have 2 patches");

    // Cherry-pick onto dest
    repo.set_current_channel("dest")?;
    let cherry_cs = cherry_pick(&repo, &source_cs, "dest", "dev")?;

    // Verify both resources were updated
    let snap_a = repo.load_snapshot("items.a")?.expect("Should have A");
    let snap_b = repo.load_snapshot("items.b")?.expect("Should have B");
    assert_eq!(snap_a["count"], 10);
    assert_eq!(snap_b["count"], 20);
    assert_eq!(cherry_cs.patches.len(), 2, "Cherry-pick should have 2 patches");

    // Verify affected resources
    let affected = cherry_cs.affected_resources();
    assert!(affected.contains(&"items.a".to_string()));
    assert!(affected.contains(&"items.b".to_string()));

    println!("Multi-resource cherry-pick verified: A.count=10, B.count=20");
    Ok(())
}

#[test]
fn test_cherry_pick_hash_integrity() -> Result<()> {
    let repo = init_memory_repo()?;

    // Create base
    repo.create_channel("src", Some("main"))?;
    repo.set_current_channel("src")?;
    let doc = json!({"value": 42});
    let _base = commit_resource(&repo, "data.x", None, &doc, "Create x", "admin")?;

    // Make a change on src
    let doc2 = json!({"value": 100});
    let src_cs = commit_resource(&repo, "data.x", Some(&doc), &doc2, "Update x", "dev")?;

    // Fork dest from src's base and make a DIVERGENT change so the
    // cherry-pick's parent_snapshot differs from the source's.
    repo.create_channel("dest", Some("main"))?;
    repo.set_current_channel("dest")?;
    let base_snap = json!({"value": 42});
    repo.save_snapshot("data.x", &base_snap)?;
    let _dest_base = commit_resource(&repo, "data.x", None, &base_snap, "Sync base", "admin")?;
    // Diverge: add an extra field on dest so the snapshot differs
    let dest_diverged = json!({"value": 42, "extra": "dest-only"});
    let _dest_div = commit_resource(
        &repo,
        "data.x",
        Some(&base_snap),
        &dest_diverged,
        "Add extra field",
        "admin",
    )?;

    // Cherry-pick src's change onto dest (which now has a different snapshot)
    let cherry_cs = cherry_pick(&repo, &src_cs, "dest", "dev")?;

    // Verify hash integrity
    assert!(cherry_cs.verify(), "Cherry-picked changeset should verify its commit_hash");
    for p in &cherry_cs.patches {
        assert!(p.verify(), "Each cherry-picked patch should verify its hash");
    }

    // Verify the cherry-pick changeset is different from the source
    assert_ne!(cherry_cs.change_id, src_cs.change_id);
    assert_ne!(cherry_cs.commit_hash, src_cs.commit_hash);

    // Verify patch hashes differ because the parent_snapshot (dest's state)
    // is different from the source's parent_snapshot.
    assert_ne!(
        cherry_cs.patches[0].hash, src_cs.patches[0].hash,
        "Cherry-picked patch hash should differ from source (different context)"
    );

    // Verify the result has both the cherry-picked change and dest's divergence
    let final_snap = repo.load_snapshot("data.x")?.expect("Should have snapshot");
    assert_eq!(final_snap["value"], 100, "Cherry-picked value should be applied");
    assert_eq!(final_snap["extra"], "dest-only", "Dest's divergent field should be preserved");

    println!("Cherry-pick hash integrity verified");
    Ok(())
}

#[test]
fn test_cherry_pick_preserves_channel_history() -> Result<()> {
    let repo = init_memory_repo()?;

    // Setup: base → feature with 2 commits → cherry-pick one onto dest
    repo.create_channel("feature", Some("main"))?;
    repo.set_current_channel("feature")?;

    let doc1 = json!({"step": 1});
    let cs1 = commit_resource(&repo, "steps.s", None, &doc1, "Step 1", "dev")?;
    let doc2 = json!({"step": 2});
    let cs2 = commit_resource(&repo, "steps.s", Some(&doc1), &doc2, "Step 2", "dev")?;
    let doc3 = json!({"step": 3});
    let cs3 = commit_resource(&repo, "steps.s", Some(&doc2), &doc3, "Step 3", "dev")?;

    // Create dest and cherry-pick only cs2 (Step 2)
    repo.create_channel("dest", Some("main"))?;
    repo.set_current_channel("dest")?;

    // Dest needs a base snapshot for the resource
    repo.save_snapshot("steps.s", &doc1)?;
    let dest_base = commit_resource(&repo, "steps.s", None, &doc1, "Sync step 1", "admin")?;

    let cherry_cs = cherry_pick(&repo, &cs2, "dest", "dev")?;

    // Verify dest channel has exactly 2 changesets: base + cherry-pick
    let dest_changesets = repo.load_channel_changesets("dest")?;
    assert_eq!(dest_changesets.len(), 2, "Dest should have 2 changesets");
    assert_eq!(dest_changesets[0].change_id, dest_base.change_id);
    assert_eq!(dest_changesets[1].change_id, cherry_cs.change_id);

    // Verify feature channel is unchanged (still has 3 changesets)
    let feature_changesets = repo.load_channel_changesets("feature")?;
    assert_eq!(feature_changesets.len(), 3, "Feature should still have 3 changesets");
    assert_eq!(feature_changesets[0].change_id, cs1.change_id);
    assert_eq!(feature_changesets[1].change_id, cs2.change_id);
    assert_eq!(feature_changesets[2].change_id, cs3.change_id);

    // Verify the cherry-pick's parent is dest's base, not feature's cs1
    assert_eq!(
        cherry_cs.parents,
        vec![dest_base.change_id.clone()],
        "Cherry-pick parent should be dest's head"
    );

    // Verify the snapshot: cherry-picking "step 1 → step 2" onto "step 1"
    // should produce step 2
    let dest_snap = repo.load_snapshot("steps.s")?.expect("Should have snapshot");
    assert_eq!(dest_snap["step"], 2, "Dest should have step=2 after cherry-pick");

    println!("Cherry-pick preserves channel history verified");
    Ok(())
}
