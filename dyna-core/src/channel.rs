//! Channel management and promotion logic.
//!
//! In the Jujutsu-inspired model, channels serve as named bookmarks pointing
//! into the changeset DAG. Each channel maintains an ordered list of changeset
//! IDs and a `head_change_id` pointing to the latest changeset.
//!
//! **Promotion** is the process of merging changesets from a source channel
//! (e.g., a feature branch) into a target channel (e.g., `main`). Promoted
//! changesets are marked as immutable to prevent history rewriting after sharing.

use crate::error::{DynaError, DynaResult};
use crate::models::{Changeset, Channel};
use itertools::Itertools;
use std::collections::HashSet;

/// Promote changesets from a source channel to a target channel.
///
/// Collects all changeset IDs in `source` that are not already in `target`
/// and appends them. Returns the list of promoted change_ids.
pub fn promote_changesets(
    source: &Channel,
    target: &mut Channel,
) -> DynaResult<Vec<String>> {
    let target_set: HashSet<&String> = target.changesets.iter().collect();

    let new_changesets = source
        .changesets
        .iter()
        .filter(|id| !target_set.contains(id))
        .cloned()
        .collect_vec();

    new_changesets
        .is_empty()
        .then(|| {
            Err(DynaError::Other(
                "No new changesets to promote. Source and target are already in sync.".into(),
            ))
        })
        .unwrap_or_else(|| {
            new_changesets
                .iter()
                .for_each(|id| target.append_changeset(id.clone()));
            Ok(new_changesets)
        })
}

/// Validate that all changesets in a channel form a valid parent chain.
///
/// Each changeset's parents must be satisfied by changesets that appear
/// earlier in the channel's list (or be empty for the root).
///
/// Uses `try_fold` to accumulate the set of available changesets while
/// validating parent dependencies at each step.
pub fn validate_channel_integrity(
    channel: &Channel,
    changesets: &std::collections::HashMap<String, Changeset>,
) -> DynaResult<()> {
    channel
        .changesets
        .iter()
        .try_fold(HashSet::new(), |mut available, change_id| {
            let cs = changesets
                .get(change_id)
                .ok_or_else(|| DynaError::Other(format!("Changeset '{}' not found", change_id)))?;

            cs.parents
                .iter()
                .try_for_each(|parent| {
                    available
                        .contains(parent)
                        .then_some(())
                        .ok_or_else(|| {
                            DynaError::DependencyMissing(change_id.clone(), parent.clone())
                        })
                })?;

            available.insert(change_id.clone());
            Ok(available)
        })
        .map(|_| ())
}

/// Get a summary of the differences between two channels.
///
/// Uses itertools `collect_vec` and set operations for clean partitioning.
pub fn channel_diff(source: &Channel, target: &Channel) -> ChannelDiffSummary {
    let source_set: HashSet<&String> = source.changesets.iter().collect();
    let target_set: HashSet<&String> = target.changesets.iter().collect();

    let (common, only_in_source): (Vec<_>, Vec<_>) = source
        .changesets
        .iter()
        .partition(|id| target_set.contains(id));

    let only_in_target = target
        .changesets
        .iter()
        .filter(|id| !source_set.contains(id))
        .cloned()
        .collect_vec();

    ChannelDiffSummary {
        only_in_source: only_in_source.into_iter().cloned().collect_vec(),
        only_in_target,
        common: common.into_iter().cloned().collect_vec(),
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
    fn test_promote_changesets() {
        let mut source = Channel::new("feature");
        source.append_changeset("aaa".into());
        source.append_changeset("bbb".into());
        source.append_changeset("ccc".into());

        let mut target = Channel::new("main");
        target.append_changeset("aaa".into());

        let promoted = promote_changesets(&source, &mut target).unwrap();
        assert_eq!(promoted, vec!["bbb", "ccc"]);
        assert_eq!(target.changesets.len(), 3);
    }

    #[test]
    fn test_promote_no_new_changesets() {
        let mut source = Channel::new("feature");
        source.append_changeset("aaa".into());

        let mut target = Channel::new("main");
        target.append_changeset("aaa".into());

        let result = promote_changesets(&source, &mut target);
        assert!(result.is_err());
    }

    #[test]
    fn test_channel_diff() {
        let mut a = Channel::new("a");
        a.append_changeset("1".into());
        a.append_changeset("2".into());
        a.append_changeset("3".into());

        let mut b = Channel::new("b");
        b.append_changeset("2".into());
        b.append_changeset("4".into());

        let diff = channel_diff(&a, &b);
        assert_eq!(diff.only_in_source, vec!["1", "3"]);
        assert_eq!(diff.only_in_target, vec!["4"]);
        assert_eq!(diff.common, vec!["2"]);
    }
}
