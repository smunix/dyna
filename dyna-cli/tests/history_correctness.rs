//! Integration test: correctness of the history / log command.
//!
//! The `dyna log` command reads the current channel's changeset list and
//! displays them in reverse chronological order. The `dyna history` command
//! queries a remote server for per-resource history, but we can test the
//! local building blocks that feed it.
//!
//! This test verifies:
//!
//! 1. A linear chain of commits produces the correct ordered log.
//! 2. The log respects channel boundaries (different channels have different
//!    histories).
//! 3. After a promote, the target channel's log includes the promoted
//!    changesets.
//! 4. Each changeset in the log has correct metadata (author, message,
//!    parent chain, patch count, operation count, affected resources).
//! 5. The `load_channel_changesets` method returns changesets in the order
//!    they were appended (oldest first), matching the channel's changeset list.
//! 6. After a revert, the log includes the revert changeset.

use anyhow::Result;
use dyna_core::channel::promote_changesets;
use dyna_core::diff::{self, invert_operations};
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn test_linear_history_order() -> Result<()> {
    let repo = init_memory_repo()?;

    // Create a chain of 5 commits
    let mut prev: Option<Value> = None;
    let mut expected_ids: Vec<String> = Vec::new();
    let mut expected_messages: Vec<String> = Vec::new();

    for i in 1..=5 {
        let doc = json!({"counter": i, "label": format!("version-{}", i)});
        let msg = format!("Commit #{}", i);
        let cs = commit_resource(
            &repo,
            "counters.main",
            prev.as_ref(),
            &doc,
            &msg,
            "test-user",
        )?;
        expected_ids.push(cs.change_id.clone());
        expected_messages.push(msg);
        prev = Some(doc);
    }

    // Load the channel's changesets
    let changesets = repo.load_channel_changesets("main")?;
    assert_eq!(changesets.len(), 5, "Should have 5 changesets");

    // Verify order: oldest first (same as channel append order)
    for (i, cs) in changesets.iter().enumerate() {
        assert_eq!(cs.change_id, expected_ids[i], "Changeset {} should match", i);
        assert_eq!(cs.message, expected_messages[i], "Message {} should match", i);
    }

    // Verify parent chain
    assert!(changesets[0].parents.is_empty(), "First changeset should have no parents");
    for i in 1..5 {
        assert_eq!(
            changesets[i].parents,
            vec![changesets[i - 1].change_id.clone()],
            "Changeset {} should have changeset {} as parent",
            i,
            i - 1
        );
    }

    // Verify the channel head points to the last changeset
    let channel = repo.load_channel("main")?;
    assert_eq!(
        channel.head_change_id.as_ref().unwrap(),
        &expected_ids[4],
        "Channel head should point to last changeset"
    );

    println!("Linear history order verified for 5 changesets");
    Ok(())
}

#[test]
fn test_history_channel_isolation() -> Result<()> {
    let repo = init_memory_repo()?;

    // Create a base commit on main
    let base_doc = json!({"name": "Base"});
    let base_cs = commit_resource(&repo, "items.base", None, &base_doc, "Base commit", "admin")?;

    // Fork a feature channel
    repo.create_channel("feature", Some("main"))?;
    repo.set_current_channel("feature")?;

    // Make 3 commits on feature
    let mut prev = base_doc.clone();
    let mut feature_ids: Vec<String> = Vec::new();
    for i in 1..=3 {
        let doc = json!({"name": "Base", "feature_field": i});
        let cs = commit_resource(
            &repo,
            "items.base",
            Some(&prev),
            &doc,
            &format!("Feature commit #{}", i),
            "dev",
        )?;
        feature_ids.push(cs.change_id.clone());
        prev = doc;
    }

    // Switch back to main and make a commit there
    repo.set_current_channel("main")?;
    let main_doc = json!({"name": "Base", "main_field": true});
    let main_cs = commit_resource(
        &repo,
        "items.base",
        Some(&base_doc),
        &main_doc,
        "Main-only commit",
        "admin",
    )?;

    // Verify main has 2 changesets (base + main-only)
    let main_changesets = repo.load_channel_changesets("main")?;
    assert_eq!(main_changesets.len(), 2);
    assert_eq!(main_changesets[0].change_id, base_cs.change_id);
    assert_eq!(main_changesets[1].change_id, main_cs.change_id);

    // Verify feature has 4 changesets (base + 3 feature)
    let feature_changesets = repo.load_channel_changesets("feature")?;
    assert_eq!(feature_changesets.len(), 4);
    assert_eq!(feature_changesets[0].change_id, base_cs.change_id);
    for i in 0..3 {
        assert_eq!(feature_changesets[i + 1].change_id, feature_ids[i]);
    }

    // The main-only commit should NOT appear in feature's history
    let feature_ids_set: std::collections::HashSet<String> = feature_changesets
        .iter()
        .map(|cs| cs.change_id.clone())
        .collect();
    assert!(
        !feature_ids_set.contains(&main_cs.change_id),
        "Feature channel should not contain main-only commit"
    );

    println!("Channel isolation verified: main has 2, feature has 4 changesets");
    Ok(())
}

#[test]
fn test_history_after_promote() -> Result<()> {
    let repo = init_memory_repo()?;

    // Base commit on setup channel
    repo.create_channel("setup", Some("main"))?;
    repo.set_current_channel("setup")?;
    let base_doc = json!({"name": "Widget", "version": 1});
    let base_cs = commit_resource(&repo, "widgets.w1", None, &base_doc, "Create widget", "admin")?;

    // Promote setup → main
    let setup_ch = repo.load_channel("setup")?;
    let mut main_ch = repo.load_channel("main")?;
    let promoted = promote_changesets(&setup_ch, &mut main_ch)?;
    repo.save_channel(&main_ch)?;
    assert_eq!(promoted.len(), 1);

    // Fork feature from main
    repo.create_channel("feature", Some("main"))?;
    repo.set_current_channel("feature")?;

    // Make 2 feature commits
    let v2 = json!({"name": "Widget", "version": 2});
    let feat_cs1 = commit_resource(
        &repo,
        "widgets.w1",
        Some(&base_doc),
        &v2,
        "Bump to v2",
        "dev",
    )?;
    let v3 = json!({"name": "Widget", "version": 3, "stable": true});
    let feat_cs2 = commit_resource(
        &repo,
        "widgets.w1",
        Some(&v2),
        &v3,
        "Bump to v3, mark stable",
        "dev",
    )?;

    // Promote feature → main
    let feature_ch = repo.load_channel("feature")?;
    let mut main_ch = repo.load_channel("main")?;
    let promoted = promote_changesets(&feature_ch, &mut main_ch)?;
    repo.save_channel(&main_ch)?;
    assert_eq!(promoted.len(), 2);

    // Verify main's history now includes all 3 changesets
    let main_changesets = repo.load_channel_changesets("main")?;
    assert_eq!(main_changesets.len(), 3);
    assert_eq!(main_changesets[0].change_id, base_cs.change_id);
    assert_eq!(main_changesets[1].change_id, feat_cs1.change_id);
    assert_eq!(main_changesets[2].change_id, feat_cs2.change_id);

    // Verify metadata
    assert_eq!(main_changesets[0].message, "Create widget");
    assert_eq!(main_changesets[1].message, "Bump to v2");
    assert_eq!(main_changesets[2].message, "Bump to v3, mark stable");

    println!("History after promote verified: main has 3 changesets in correct order");
    Ok(())
}

#[test]
fn test_history_metadata_correctness() -> Result<()> {
    let repo = init_memory_repo()?;

    // Create a commit with known metadata
    let doc = json!({"x": 1, "y": 2, "z": 3});
    let cs = commit_resource(&repo, "coords.point", None, &doc, "Add point", "alice")?;

    // Load and verify all metadata fields
    let loaded = repo.load_changeset(&cs.change_id)?;
    assert_eq!(loaded.change_id, cs.change_id);
    assert_eq!(loaded.commit_hash, cs.commit_hash);
    assert_eq!(loaded.message, "Add point");
    assert_eq!(loaded.author, "alice");
    assert!(loaded.parents.is_empty(), "Root changeset should have no parents");
    assert!(!loaded.immutable, "New changeset should be mutable");
    assert!(!loaded.empty, "Changeset with patches should not be empty");
    assert_eq!(loaded.patches.len(), 1, "Should have 1 patch");
    assert_eq!(loaded.patches[0].target_resource, "coords.point");
    assert!(loaded.patches[0].operations.len() > 0, "Patch should have operations");
    assert!(loaded.verify(), "Changeset should verify its hash");

    // Verify affected_resources
    let affected = loaded.affected_resources();
    assert_eq!(affected, vec!["coords.point"]);

    // Verify total_operations
    let total_ops = loaded.total_operations();
    assert!(total_ops > 0, "Should have at least 1 operation");

    // Now create a second commit and verify parent linkage
    let doc2 = json!({"x": 10, "y": 20, "z": 30});
    let cs2 = commit_resource(
        &repo,
        "coords.point",
        Some(&doc),
        &doc2,
        "Update point",
        "bob",
    )?;
    let loaded2 = repo.load_changeset(&cs2.change_id)?;
    assert_eq!(loaded2.parents, vec![cs.change_id.clone()]);
    assert_eq!(loaded2.author, "bob");
    assert_eq!(loaded2.message, "Update point");

    // Verify the patch records the correct parent/result snapshots
    let patch = &loaded2.patches[0];
    assert_eq!(patch.parent_snapshot.as_ref().unwrap(), &doc);
    assert_eq!(patch.result_snapshot.as_ref().unwrap(), &doc2);

    println!("Metadata correctness verified for 2 changesets");
    Ok(())
}

#[test]
fn test_history_with_revert() -> Result<()> {
    let repo = init_memory_repo()?;

    // Create a feature channel (revert is not allowed on main)
    repo.create_channel("feature", Some("main"))?;
    repo.set_current_channel("feature")?;

    // Commit a document
    let doc1 = json!({"name": "Item", "active": true});
    let cs1 = commit_resource(&repo, "items.one", None, &doc1, "Create item", "admin")?;

    // Modify it
    let doc2 = json!({"name": "Item", "active": false, "reason": "deprecated"});
    let cs2 = commit_resource(
        &repo,
        "items.one",
        Some(&doc1),
        &doc2,
        "Deprecate item",
        "admin",
    )?;

    // Now revert cs2 by building inverse patches (simulating `dyna revert`)
    let target_cs = repo.load_changeset(&cs2.change_id)?;
    let inverse_patches: Vec<Patch> = target_cs
        .patches
        .iter()
        .map(|original| {
            let base = original
                .parent_snapshot
                .as_ref()
                .cloned()
                .unwrap_or_else(|| json!({}));
            let inverse_ops = invert_operations(&original.operations, &base);
            Patch::new(
                original.target_resource.clone(),
                inverse_ops,
                original.result_snapshot.clone(),
                original.parent_snapshot.clone(),
            )
        })
        .collect();

    let channel = repo.load_channel("feature")?;
    let parents = channel
        .head_change_id
        .map(|id| vec![id])
        .unwrap_or_default();
    let revert_cs = Changeset::new(
        "admin".to_string(),
        format!("Revert \"{}\" ({})", cs2.message, cs2.short_change_id()),
        parents,
        inverse_patches,
    );
    repo.store_changeset(&revert_cs)?;

    // Update snapshot to reverted state
    for p in &revert_cs.patches {
        if let Some(ref snap) = p.result_snapshot {
            repo.save_snapshot(&p.target_resource, snap)?;
        }
    }

    // Update channel
    let mut channel = repo.load_channel("feature")?;
    channel.append_changeset(revert_cs.change_id.clone());
    repo.save_channel(&channel)?;

    // Verify history has 3 entries
    let changesets = repo.load_channel_changesets("feature")?;
    assert_eq!(changesets.len(), 3, "Should have 3 changesets (create + deprecate + revert)");
    assert_eq!(changesets[0].change_id, cs1.change_id);
    assert_eq!(changesets[1].change_id, cs2.change_id);
    assert_eq!(changesets[2].change_id, revert_cs.change_id);

    // Verify the revert message
    assert!(
        changesets[2].message.contains("Revert"),
        "Revert changeset message should contain 'Revert'"
    );
    assert!(
        changesets[2].message.contains("Deprecate item"),
        "Revert message should reference original message"
    );

    // Verify the snapshot is back to the original state
    let final_snap = repo
        .load_snapshot("items.one")?
        .expect("Should have snapshot");
    assert_eq!(final_snap, doc1, "Snapshot should be reverted to original state");

    println!("History with revert verified: 3 changesets, snapshot reverted");
    Ok(())
}

#[test]
fn test_history_changeset_lookup_by_prefix() -> Result<()> {
    let repo = init_memory_repo()?;

    // Create several changesets
    let doc1 = json!({"a": 1});
    let cs1 = commit_resource(&repo, "data.a", None, &doc1, "First", "user")?;
    let doc2 = json!({"b": 2});
    let cs2 = commit_resource(&repo, "data.b", None, &doc2, "Second", "user")?;
    let doc3 = json!({"c": 3});
    let cs3 = commit_resource(&repo, "data.c", None, &doc3, "Third", "user")?;

    // Look up by full ID
    let found = repo.load_changeset(&cs2.change_id)?;
    assert_eq!(found.change_id, cs2.change_id);

    // Look up by prefix (first 8 chars)
    let prefix = &cs1.change_id[..8];
    let matches = repo.find_changeset_by_prefix(prefix)?;
    assert!(
        matches.len() >= 1,
        "Should find at least 1 match for 8-char prefix"
    );
    assert!(
        matches.iter().any(|m| m.change_id == cs1.change_id),
        "Prefix match should include the target changeset"
    );

    // Look up by very short prefix (might match multiple)
    let short_prefix = &cs1.change_id[..2];
    let short_matches = repo.find_changeset_by_prefix(short_prefix)?;
    assert!(
        short_matches.len() >= 1,
        "Even a 2-char prefix should find at least 1 match"
    );

    println!(
        "Prefix lookup verified: full={}, 8-char={} matches, 2-char={} matches",
        cs2.short_change_id(),
        matches.len(),
        short_matches.len()
    );
    Ok(())
}
