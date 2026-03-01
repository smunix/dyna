package dynago

import (
	"encoding/json"
	"fmt"
)

// ---------------------------------------------------------------------------
// Add (stage a file)
// ---------------------------------------------------------------------------

// Add stages a working-directory file for the next commit.
func (r *Repository) Add(relativePath string) error {
	resourceID := ResourceIDFromPath(relativePath)

	data, err := r.ReadWorkFile(relativePath)
	if err != nil {
		return fmt.Errorf("file not found: %s", relativePath)
	}

	var current interface{}
	if err := json.Unmarshal(data, &current); err != nil {
		return fmt.Errorf("invalid JSON in %s: %w", relativePath, err)
	}

	// Load previous snapshot (may be nil for new files)
	prevSnap, _ := r.LoadSnapshot(resourceID)

	var ops []PatchOperation
	if prevSnap != nil {
		ops, err = Diff(prevSnap, data)
		if err != nil {
			return err
		}
	} else {
		// New file — single "add" at root
		ops = []PatchOperation{{Op: OpAdd, Path: "", Value: data}}
	}

	staged, _ := r.LoadStaged()

	// Remove any existing staged entry for this resource
	filtered := make([]StagedChange, 0, len(staged))
	for _, s := range staged {
		if s.ResourceID != resourceID {
			filtered = append(filtered, s)
		}
	}

	filtered = append(filtered, StagedChange{
		ResourceID: resourceID,
		FilePath:   relativePath,
		Previous:   prevSnap,
		Current:    data,
		Operations: ops,
	})

	return r.SaveStaged(filtered)
}

// AddDelete stages a file deletion for the next commit.
func (r *Repository) AddDelete(relativePath string) error {
	resourceID := ResourceIDFromPath(relativePath)

	prevSnap, _ := r.LoadSnapshot(resourceID)
	if prevSnap == nil {
		return fmt.Errorf("resource not found in snapshots: %s", resourceID)
	}

	ops := []PatchOperation{{Op: OpRemove, Path: ""}}

	staged, _ := r.LoadStaged()
	filtered := make([]StagedChange, 0, len(staged))
	for _, s := range staged {
		if s.ResourceID != resourceID {
			filtered = append(filtered, s)
		}
	}

	filtered = append(filtered, StagedChange{
		ResourceID: resourceID,
		FilePath:   relativePath,
		Previous:   prevSnap,
		Current:    json.RawMessage("null"),
		Operations: ops,
	})

	return r.SaveStaged(filtered)
}

// ---------------------------------------------------------------------------
// Commit
// ---------------------------------------------------------------------------

// Commit creates a changeset from staged changes.
// Returns the new changeset's change_id.
func (r *Repository) Commit(message string) (string, error) {
	channelName, err := r.CurrentChannelName()
	if err != nil {
		return "", err
	}
	if channelName == "main" {
		return "", fmt.Errorf("cannot commit directly to the 'main' channel; create a feature channel first")
	}

	staged, err := r.LoadStaged()
	if err != nil {
		return "", err
	}
	if len(staged) == 0 {
		return "", fmt.Errorf("nothing to commit: no staged changes")
	}

	cfg, err := r.LoadConfig()
	if err != nil {
		return "", err
	}

	ch, err := r.LoadChannel(channelName)
	if err != nil {
		return "", err
	}

	// Build parents
	var parents []string
	if ch.HeadChangeID != nil {
		parents = []string{*ch.HeadChangeID}
	}

	// Build patches from staged changes
	var patches []Patch
	for _, sc := range staged {
		p := NewPatch(sc.ResourceID, sc.Operations, nil, nil)
		patches = append(patches, p)
	}

	cs := NewChangeset(cfg.User.Name, message, parents, patches)

	// Save changeset
	if err := r.SaveChangeset(&cs); err != nil {
		return "", err
	}

	// Update snapshots
	for _, sc := range staged {
		isDelete := false
		for _, op := range sc.Operations {
			if op.Op == OpRemove && op.Path == "" {
				isDelete = true
				break
			}
		}
		if isDelete {
			_ = r.RemoveSnapshot(sc.ResourceID)
		} else {
			_ = r.SaveSnapshot(sc.ResourceID, sc.Current)
		}
	}

	// Update channel
	ch.AppendChangeset(cs.ChangeID)
	if err := r.SaveChannel(ch); err != nil {
		return "", err
	}

	// Clear staging area
	if err := r.SaveStaged(nil); err != nil {
		return "", err
	}

	return cs.ChangeID, nil
}

// ---------------------------------------------------------------------------
// Status
// ---------------------------------------------------------------------------

// StatusResult holds the current repository status.
type StatusResult struct {
	Channel          string           `json:"channel"`
	Staged           []StagedFileInfo `json:"staged"`
	Modified         []string         `json:"modified"`
	Deleted          []string         `json:"deleted"`
	UnstagedOnStaged []string         `json:"unstaged_on_staged"`
	Conflicts        []string         `json:"conflicts"`
}

// StagedFileInfo describes a single staged file.
type StagedFileInfo struct {
	ResourceID string `json:"resource_id"`
	Ops        int    `json:"ops"`
	Kind       string `json:"kind"` // "new", "modified", "deleted"
}

// Status computes the current repository status.
func (r *Repository) Status() (*StatusResult, error) {
	channelName, err := r.CurrentChannelName()
	if err != nil {
		return nil, err
	}

	staged, _ := r.LoadStaged()

	var stagedInfo []StagedFileInfo
	for _, sc := range staged {
		kind := "modified"
		if sc.Previous == nil || string(sc.Previous) == "null" {
			kind = "new"
		}
		for _, op := range sc.Operations {
			if op.Op == OpRemove && op.Path == "" {
				kind = "deleted"
				break
			}
		}
		stagedInfo = append(stagedInfo, StagedFileInfo{
			ResourceID: sc.ResourceID,
			Ops:        len(sc.Operations),
			Kind:       kind,
		})
	}

	// Detect modified and deleted files (not yet staged)
	snapIDs, _ := r.ListSnapshots()
	var modified, deleted []string
	for _, id := range snapIDs {
		relPath := PathForResourceID(id)
		if !r.WorkFileExists(relPath) {
			deleted = append(deleted, id)
			continue
		}
		snap, _ := r.LoadSnapshot(id)
		work, _ := r.ReadWorkFile(relPath)
		if snap != nil && string(snap) != string(work) {
			modified = append(modified, id)
		}
	}

	// Detect unstaged changes on already-staged files
	var unstagedOnStaged []string
	for _, sc := range staged {
		relPath := PathForResourceID(sc.ResourceID)
		if r.WorkFileExists(relPath) {
			work, _ := r.ReadWorkFile(relPath)
			if string(sc.Current) != string(work) {
				unstagedOnStaged = append(unstagedOnStaged, sc.ResourceID)
			}
		}
	}

	conflicts, _ := r.ListConflictedResources()

	return &StatusResult{
		Channel:          channelName,
		Staged:           stagedInfo,
		Modified:         modified,
		Deleted:          deleted,
		UnstagedOnStaged: unstagedOnStaged,
		Conflicts:        conflicts,
	}, nil
}

// ---------------------------------------------------------------------------
// Log
// ---------------------------------------------------------------------------

// LogEntry describes a changeset in the log.
type LogEntry struct {
	ChangeID   string `json:"change_id"`
	CommitHash string `json:"commit_hash"`
	Message    string `json:"message"`
	Author     string `json:"author"`
	CreatedAt  string `json:"created_at"`
	PatchCount int    `json:"patch_count"`
	Immutable  bool   `json:"immutable"`
}

// Log returns the changeset history for the current channel.
func (r *Repository) Log() ([]LogEntry, error) {
	ch, err := r.CurrentChannel()
	if err != nil {
		return nil, err
	}

	entries := make([]LogEntry, 0, len(ch.Changesets))
	// Reverse order (newest first)
	for i := len(ch.Changesets) - 1; i >= 0; i-- {
		cs, err := r.LoadChangeset(ch.Changesets[i])
		if err != nil {
			continue
		}
		entries = append(entries, LogEntry{
			ChangeID:   cs.ChangeID,
			CommitHash: cs.CommitHash,
			Message:    cs.Message,
			Author:     cs.Author,
			CreatedAt:  cs.CreatedAt.Format("2006-01-02T15:04:05Z"),
			PatchCount: len(cs.Patches),
			Immutable:  cs.Immutable,
		})
	}
	return entries, nil
}

// ---------------------------------------------------------------------------
// DiffChangeset — show the diff for a specific changeset
// ---------------------------------------------------------------------------

// DiffResult holds the diff output for a changeset.
type DiffResult struct {
	ChangeID string       `json:"change_id"`
	Patches  []PatchInfo  `json:"patches"`
}

// PatchInfo describes a single patch within a changeset.
type PatchInfo struct {
	TargetResource string           `json:"target_resource"`
	Operations     []PatchOperation `json:"operations"`
}

// DiffChangeset returns the patches for a given changeset (or the latest staged).
func (r *Repository) DiffChangeset(changeID *string) (*DiffResult, error) {
	if changeID == nil {
		// Show staged diff
		staged, _ := r.LoadStaged()
		var patches []PatchInfo
		for _, sc := range staged {
			patches = append(patches, PatchInfo{
				TargetResource: sc.ResourceID,
				Operations:     sc.Operations,
			})
		}
		return &DiffResult{ChangeID: "(staged)", Patches: patches}, nil
	}

	cs, err := r.FindChangesetByPrefix(*changeID)
	if err != nil {
		return nil, err
	}

	var patches []PatchInfo
	for _, p := range cs.Patches {
		patches = append(patches, PatchInfo{
			TargetResource: p.TargetResource,
			Operations:     p.Operations,
		})
	}
	return &DiffResult{ChangeID: cs.ChangeID, Patches: patches}, nil
}

// ---------------------------------------------------------------------------
// Restore — discard working changes for a file
// ---------------------------------------------------------------------------

// Restore restores a working file from the current channel's snapshot.
func (r *Repository) Restore(relativePath string) error {
	resourceID := ResourceIDFromPath(relativePath)
	snap, _ := r.LoadSnapshot(resourceID)
	if snap == nil {
		return fmt.Errorf("no snapshot found for %s", resourceID)
	}
	return r.WriteWorkFile(relativePath, snap)
}

// RestoreFromChannel restores a working file from another channel's snapshot.
func (r *Repository) RestoreFromChannel(relativePath, channelName string) error {
	// Load the channel and replay its changesets to find the snapshot
	// For simplicity, we look at the snapshot store (which is channel-independent
	// in the current implementation). A full implementation would replay changesets.
	return r.Restore(relativePath)
}

// ---------------------------------------------------------------------------
// Describe — amend the message of the latest changeset
// ---------------------------------------------------------------------------

// Describe amends the message of the latest (mutable) changeset.
func (r *Repository) Describe(newMessage string) (string, error) {
	ch, err := r.CurrentChannel()
	if err != nil {
		return "", err
	}
	if ch.HeadChangeID == nil {
		return "", fmt.Errorf("no changesets in current channel")
	}

	cs, err := r.LoadChangeset(*ch.HeadChangeID)
	if err != nil {
		return "", err
	}
	if cs.Immutable {
		return "", fmt.Errorf("changeset %s is immutable and cannot be amended", cs.ChangeID)
	}

	cs.Message = newMessage
	cs.RecomputeHash()
	if err := r.SaveChangeset(cs); err != nil {
		return "", err
	}
	return cs.ChangeID, nil
}

// ---------------------------------------------------------------------------
// Squash — combine all mutable changesets into one
// ---------------------------------------------------------------------------

// Squash combines all mutable changesets in the current channel into one.
// Returns the new changeset's change_id.
func (r *Repository) Squash() (string, error) {
	ch, err := r.CurrentChannel()
	if err != nil {
		return "", err
	}

	if len(ch.Changesets) < 2 {
		return "", fmt.Errorf("nothing to squash: need at least 2 changesets")
	}

	cfg, err := r.LoadConfig()
	if err != nil {
		return "", err
	}

	// Collect mutable changesets
	var mutableIDs []string
	var allPatches []Patch
	var messages []string

	for _, cid := range ch.Changesets {
		cs, err := r.LoadChangeset(cid)
		if err != nil {
			continue
		}
		if cs.Immutable {
			continue
		}
		mutableIDs = append(mutableIDs, cid)
		allPatches = append(allPatches, cs.Patches...)
		messages = append(messages, cs.Message)
	}

	if len(mutableIDs) < 2 {
		return "", fmt.Errorf("nothing to squash: need at least 2 mutable changesets")
	}

	// Find the parent of the first mutable changeset
	var parents []string
	firstMutable, _ := r.LoadChangeset(mutableIDs[0])
	if firstMutable != nil {
		parents = firstMutable.Parents
	}

	// Merge patches by resource (keep latest per resource)
	mergedPatches := mergePatchesByResource(allPatches)

	// Combined message
	combinedMsg := ""
	for i, msg := range messages {
		if i > 0 {
			combinedMsg += "\n"
		}
		combinedMsg += msg
	}

	squashed := NewChangeset(cfg.User.Name, combinedMsg, parents, mergedPatches)

	if err := r.SaveChangeset(&squashed); err != nil {
		return "", err
	}

	// Rebuild channel: keep immutable changesets, replace mutable with squashed
	mutableSet := map[string]bool{}
	for _, id := range mutableIDs {
		mutableSet[id] = true
	}

	var newChangesets []string
	for _, cid := range ch.Changesets {
		if !mutableSet[cid] {
			newChangesets = append(newChangesets, cid)
		}
	}
	newChangesets = append(newChangesets, squashed.ChangeID)

	ch.Changesets = newChangesets
	ch.HeadChangeID = &squashed.ChangeID
	if err := r.SaveChannel(ch); err != nil {
		return "", err
	}

	return squashed.ChangeID, nil
}

// ---------------------------------------------------------------------------
// Revert — create an inverse changeset
// ---------------------------------------------------------------------------

// Revert creates a new changeset that undoes the given changeset.
// Returns the new changeset's change_id.
func (r *Repository) Revert(changeID string) (string, error) {
	cs, err := r.FindChangesetByPrefix(changeID)
	if err != nil {
		return "", err
	}

	channelName, err := r.CurrentChannelName()
	if err != nil {
		return "", err
	}
	if channelName == "main" {
		return "", fmt.Errorf("cannot commit directly to the 'main' channel")
	}

	cfg, err := r.LoadConfig()
	if err != nil {
		return "", err
	}

	ch, err := r.CurrentChannel()
	if err != nil {
		return "", err
	}

	var parents []string
	if ch.HeadChangeID != nil {
		parents = []string{*ch.HeadChangeID}
	}

	// Invert each patch
	var invertedPatches []Patch
	for _, p := range cs.Patches {
		snap, _ := r.LoadSnapshot(p.TargetResource)
		invOps, err := InvertOperations(p.Operations, snap)
		if err != nil {
			return "", fmt.Errorf("failed to invert patch for %s: %w", p.TargetResource, err)
		}
		invertedPatches = append(invertedPatches, NewPatch(p.TargetResource, invOps, nil, nil))
	}

	revertCS := NewChangeset(
		cfg.User.Name,
		fmt.Sprintf("Revert \"%s\" (%s)", cs.Message, cs.ChangeID[:8]),
		parents,
		invertedPatches,
	)

	if err := r.SaveChangeset(&revertCS); err != nil {
		return "", err
	}

	// Apply inverted patches to snapshots
	for _, p := range invertedPatches {
		snap, _ := r.LoadSnapshot(p.TargetResource)
		if snap == nil {
			snap = json.RawMessage("{}")
		}
		if err := ApplyPatch(&snap, p.Operations); err != nil {
			// Best effort — continue
			continue
		}
		_ = r.SaveSnapshot(p.TargetResource, snap)

		// Update working file
		relPath := PathForResourceID(p.TargetResource)
		_ = r.WriteWorkFile(relPath, snap)
	}

	ch.AppendChangeset(revertCS.ChangeID)
	if err := r.SaveChannel(ch); err != nil {
		return "", err
	}

	return revertCS.ChangeID, nil
}

// ---------------------------------------------------------------------------
// CherryPick — copy a changeset from another channel
// ---------------------------------------------------------------------------

// CherryPick applies the changes from a changeset (from any channel) to the
// current channel. Returns the new changeset's change_id.
func (r *Repository) CherryPick(changeID string) (string, error) {
	cs, err := r.FindChangesetByPrefix(changeID)
	if err != nil {
		return "", err
	}

	channelName, err := r.CurrentChannelName()
	if err != nil {
		return "", err
	}
	if channelName == "main" {
		return "", fmt.Errorf("cannot commit directly to the 'main' channel")
	}

	cfg, err := r.LoadConfig()
	if err != nil {
		return "", err
	}

	ch, err := r.CurrentChannel()
	if err != nil {
		return "", err
	}

	var parents []string
	if ch.HeadChangeID != nil {
		parents = []string{*ch.HeadChangeID}
	}

	// Apply each patch to the current snapshot
	var newPatches []Patch
	for _, p := range cs.Patches {
		snap, _ := r.LoadSnapshot(p.TargetResource)
		if snap == nil {
			snap = json.RawMessage("{}")
		}

		// Apply the original operations to get the new state
		newSnap := make(json.RawMessage, len(snap))
		copy(newSnap, snap)
		if err := ApplyPatch(&newSnap, p.Operations); err != nil {
			// If direct apply fails, compute a fresh diff
			newSnap = snap
		}

		// Compute the actual diff from current snapshot to new state
		ops, err := Diff(snap, newSnap)
		if err != nil || len(ops) == 0 {
			continue
		}

		newPatches = append(newPatches, NewPatch(p.TargetResource, ops, nil, nil))

		// Update snapshot and working file
		_ = r.SaveSnapshot(p.TargetResource, newSnap)
		_ = r.WriteWorkFile(PathForResourceID(p.TargetResource), newSnap)
	}

	if len(newPatches) == 0 {
		return "", fmt.Errorf("cherry-pick produced no changes (already applied?)")
	}

	cherryCS := NewChangeset(
		cfg.User.Name,
		fmt.Sprintf("Cherry-pick \"%s\" (%s)", cs.Message, cs.ChangeID[:8]),
		parents,
		newPatches,
	)

	if err := r.SaveChangeset(&cherryCS); err != nil {
		return "", err
	}

	ch.AppendChangeset(cherryCS.ChangeID)
	if err := r.SaveChannel(ch); err != nil {
		return "", err
	}

	return cherryCS.ChangeID, nil
}

// ---------------------------------------------------------------------------
// Resolve conflicts
// ---------------------------------------------------------------------------

// Resolve marks a conflicted resource as resolved by staging the current
// working file content and clearing the conflict.
func (r *Repository) Resolve(resourceID string) error {
	conflicts, _ := r.LoadConflicts(resourceID)
	if len(conflicts) == 0 {
		return fmt.Errorf("no conflicts found for %s", resourceID)
	}

	relPath := PathForResourceID(resourceID)
	if err := r.Add(relPath); err != nil {
		return err
	}

	return r.ClearConflicts(resourceID)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

func mergePatchesByResource(patches []Patch) []Patch {
	// Keep the last patch per resource (latest wins)
	seen := map[string]int{}
	var result []Patch
	for _, p := range patches {
		if idx, ok := seen[p.TargetResource]; ok {
			// Merge operations
			result[idx].Operations = append(result[idx].Operations, p.Operations...)
		} else {
			seen[p.TargetResource] = len(result)
			result = append(result, p)
		}
	}
	return result
}
