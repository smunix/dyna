//! Channel management logic.
//!
//! Channels are named sequences of patches, analogous to branches in Git or
//! channels in Pijul. This module provides utilities for managing channels,
//! including promotion (merging one channel into another).

use crate::models::{Channel, Patch};
use crate::error::{DynaError, DynaResult};

/// Promote patches from a source channel to a target channel.
///
/// This collects all patches in `source` that are not already in `target`
/// and appends them to `target`. Returns the list of promoted patch hashes.
pub fn promote_patches(
    source: &Channel,
    target: &mut Channel,
) -> DynaResult<Vec<String>> {
    let target_set: std::collections::HashSet<&String> = target.patches.iter().collect();

    let new_patches: Vec<String> = source
        .patches
        .iter()
        .filter(|h| !target_set.contains(h))
        .cloned()
        .collect();

    if new_patches.is_empty() {
        return Err(DynaError::Other(
            "No new patches to promote. Source and target are already in sync.".into(),
        ));
    }

    for hash in &new_patches {
        target.append_patch(hash.clone());
    }

    Ok(new_patches)
}

/// Validate that all patches in a channel form a valid dependency chain.
///
/// Each patch's dependencies must be satisfied by patches that appear earlier
/// in the channel's patch list.
pub fn validate_channel_integrity(
    channel: &Channel,
    patches: &std::collections::HashMap<String, Patch>,
) -> DynaResult<()> {
    let mut available: std::collections::HashSet<String> = std::collections::HashSet::new();

    for hash in &channel.patches {
        let patch = patches
            .get(hash)
            .ok_or_else(|| DynaError::PatchNotFound(hash.clone()))?;

        for dep in &patch.dependencies {
            if !available.contains(dep) {
                return Err(DynaError::DependencyMissing(hash.clone(), dep.clone()));
            }
        }

        available.insert(hash.clone());
    }

    Ok(())
}

/// Get a summary of the differences between two channels.
pub fn channel_diff(source: &Channel, target: &Channel) -> ChannelDiffSummary {
    let source_set: std::collections::HashSet<&String> = source.patches.iter().collect();
    let target_set: std::collections::HashSet<&String> = target.patches.iter().collect();

    let only_in_source: Vec<String> = source
        .patches
        .iter()
        .filter(|h| !target_set.contains(h))
        .cloned()
        .collect();

    let only_in_target: Vec<String> = target
        .patches
        .iter()
        .filter(|h| !source_set.contains(h))
        .cloned()
        .collect();

    let common: Vec<String> = source
        .patches
        .iter()
        .filter(|h| target_set.contains(h))
        .cloned()
        .collect();

    ChannelDiffSummary {
        only_in_source,
        only_in_target,
        common,
    }
}

/// Summary of differences between two channels.
#[derive(Debug, Clone)]
pub struct ChannelDiffSummary {
    pub only_in_source: Vec<String>,
    pub only_in_target: Vec<String>,
    pub common: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Channel;

    #[test]
    fn test_promote_patches() {
        let mut source = Channel::new("feature");
        source.append_patch("sha256:aaa".into());
        source.append_patch("sha256:bbb".into());
        source.append_patch("sha256:ccc".into());

        let mut target = Channel::new("main");
        target.append_patch("sha256:aaa".into());

        let promoted = promote_patches(&source, &mut target).unwrap();
        assert_eq!(promoted, vec!["sha256:bbb", "sha256:ccc"]);
        assert_eq!(target.patches.len(), 3);
    }

    #[test]
    fn test_promote_no_new_patches() {
        let mut source = Channel::new("feature");
        source.append_patch("sha256:aaa".into());

        let mut target = Channel::new("main");
        target.append_patch("sha256:aaa".into());

        let result = promote_patches(&source, &mut target);
        assert!(result.is_err());
    }

    #[test]
    fn test_channel_diff() {
        let mut a = Channel::new("a");
        a.append_patch("sha256:1".into());
        a.append_patch("sha256:2".into());
        a.append_patch("sha256:3".into());

        let mut b = Channel::new("b");
        b.append_patch("sha256:2".into());
        b.append_patch("sha256:4".into());

        let diff = channel_diff(&a, &b);
        assert_eq!(diff.only_in_source, vec!["sha256:1", "sha256:3"]);
        assert_eq!(diff.only_in_target, vec!["sha256:4"]);
        assert_eq!(diff.common, vec!["sha256:2"]);
    }
}
