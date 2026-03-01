// Package dynago provides a pure-Go client for the Dyna distributed CRUD
// system. It uses an afero.Fs (typically afero.MemMapFs) for all local
// repository state, making it suitable for embedding in Go applications
// without touching the real filesystem.
package dynago

import (
	"crypto/rand"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"time"
)

// ---------------------------------------------------------------------------
// Patch operations (RFC 6902 JSON Patch)
// ---------------------------------------------------------------------------

// OpType enumerates the JSON Patch operation types.
type OpType string

const (
	OpAdd     OpType = "add"
	OpRemove  OpType = "remove"
	OpReplace OpType = "replace"
	OpMove    OpType = "move"
	OpCopy    OpType = "copy"
	OpTest    OpType = "test"
)

// PatchOperation is a single RFC 6902 operation.
type PatchOperation struct {
	Op    OpType          `json:"op"`
	Path  string          `json:"path"`
	Value json.RawMessage `json:"value,omitempty"`
	From  string          `json:"from,omitempty"`
}

// ---------------------------------------------------------------------------
// Patch
// ---------------------------------------------------------------------------

// PatchContent is the hashable content of a Patch.
type PatchContent struct {
	TargetResource string           `json:"target_resource"`
	Operations     []PatchOperation `json:"operations"`
	BaseHash       *string          `json:"base_hash"`
	ResultHash     *string          `json:"result_hash"`
}

// Patch represents a set of JSON Patch operations targeting a single resource.
type Patch struct {
	TargetResource string           `json:"target_resource"`
	Operations     []PatchOperation `json:"operations"`
	BaseHash       *string          `json:"base_hash"`
	ResultHash     *string          `json:"result_hash"`
	Hash           string           `json:"hash"`
	CreatedAt      time.Time        `json:"created_at"`
}

// NewPatch creates a Patch, computing its content hash.
func NewPatch(targetResource string, ops []PatchOperation, baseHash, resultHash *string) Patch {
	content := PatchContent{
		TargetResource: targetResource,
		Operations:     ops,
		BaseHash:       baseHash,
		ResultHash:     resultHash,
	}
	data, _ := json.Marshal(content)
	return Patch{
		TargetResource: targetResource,
		Operations:     ops,
		BaseHash:       baseHash,
		ResultHash:     resultHash,
		Hash:           ContentHash(data),
		CreatedAt:      time.Now().UTC(),
	}
}

// Verify checks that the patch hash matches its content.
func (p *Patch) Verify() bool {
	content := PatchContent{
		TargetResource: p.TargetResource,
		Operations:     p.Operations,
		BaseHash:       p.BaseHash,
		ResultHash:     p.ResultHash,
	}
	data, _ := json.Marshal(content)
	return ContentHash(data) == p.Hash
}

// ---------------------------------------------------------------------------
// Changeset
// ---------------------------------------------------------------------------

// ChangesetContent is the hashable content of a Changeset.
type ChangesetContent struct {
	ChangeID    string   `json:"change_id"`
	Message     string   `json:"message"`
	Author      string   `json:"author"`
	Parents     []string `json:"parents"`
	PatchHashes []string `json:"patch_hashes"`
}

// Changeset groups one or more Patches committed together.
type Changeset struct {
	ChangeID   string    `json:"change_id"`
	CommitHash string    `json:"commit_hash"`
	Message    string    `json:"message"`
	Author     string    `json:"author"`
	Parents    []string  `json:"parents"`
	Patches    []Patch   `json:"patches"`
	CreatedAt  time.Time `json:"created_at"`
	Empty      bool      `json:"empty"`
	Immutable  bool      `json:"immutable"`
}

// NewChangeset creates a Changeset with a random change_id and computed hash.
func NewChangeset(author, message string, parents []string, patches []Patch) Changeset {
	changeID := GenerateChangeID()
	now := time.Now().UTC()
	empty := len(patches) == 0

	patchHashes := make([]string, len(patches))
	for i, p := range patches {
		patchHashes[i] = p.Hash
	}

	content := ChangesetContent{
		ChangeID:    changeID,
		Message:     message,
		Author:      author,
		Parents:     parents,
		PatchHashes: patchHashes,
	}
	data, _ := json.Marshal(content)
	commitHash := ContentHash(data)

	return Changeset{
		ChangeID:   changeID,
		CommitHash: commitHash,
		Message:    message,
		Author:     author,
		Parents:    parents,
		Patches:    patches,
		CreatedAt:  now,
		Empty:      empty,
		Immutable:  false,
	}
}

// Verify checks that the changeset commit_hash matches its content.
func (cs *Changeset) Verify() bool {
	patchHashes := make([]string, len(cs.Patches))
	for i, p := range cs.Patches {
		patchHashes[i] = p.Hash
	}
	content := ChangesetContent{
		ChangeID:    cs.ChangeID,
		Message:     cs.Message,
		Author:      cs.Author,
		Parents:     cs.Parents,
		PatchHashes: patchHashes,
	}
	data, _ := json.Marshal(content)
	return ContentHash(data) == cs.CommitHash
}

// RecomputeHash recalculates the commit_hash after modifications.
func (cs *Changeset) RecomputeHash() {
	patchHashes := make([]string, len(cs.Patches))
	for i, p := range cs.Patches {
		patchHashes[i] = p.Hash
	}
	content := ChangesetContent{
		ChangeID:    cs.ChangeID,
		Message:     cs.Message,
		Author:      cs.Author,
		Parents:     cs.Parents,
		PatchHashes: patchHashes,
	}
	data, _ := json.Marshal(content)
	cs.CommitHash = ContentHash(data)
}

// TotalOperations returns the total number of patch operations.
func (cs *Changeset) TotalOperations() int {
	total := 0
	for _, p := range cs.Patches {
		total += len(p.Operations)
	}
	return total
}

// AffectedResources returns unique resource IDs affected by this changeset.
func (cs *Changeset) AffectedResources() []string {
	seen := map[string]bool{}
	var result []string
	for _, p := range cs.Patches {
		if !seen[p.TargetResource] {
			seen[p.TargetResource] = true
			result = append(result, p.TargetResource)
		}
	}
	return result
}

// ShortHash returns the first 8 characters of the commit hash (after "sha256:").
func (cs *Changeset) ShortHash() string {
	h := cs.CommitHash
	if len(h) > 7 && h[:7] == "sha256:" {
		h = h[7:]
	}
	if len(h) > 8 {
		return h[:8]
	}
	return h
}

// ---------------------------------------------------------------------------
// Channel
// ---------------------------------------------------------------------------

// Channel is a named bookmark pointing to an ordered sequence of changeset IDs.
type Channel struct {
	Name         string    `json:"name"`
	HeadChangeID *string   `json:"head_change_id"`
	Changesets   []string  `json:"changesets"`
	CreatedAt    time.Time `json:"created_at"`
	UpdatedAt    time.Time `json:"updated_at"`
}

// NewChannel creates a new empty channel.
func NewChannel(name string) Channel {
	now := time.Now().UTC()
	return Channel{
		Name:         name,
		HeadChangeID: nil,
		Changesets:   []string{},
		CreatedAt:    now,
		UpdatedAt:    now,
	}
}

// AppendChangeset adds a changeset ID to this channel.
func (ch *Channel) AppendChangeset(changeID string) {
	ch.HeadChangeID = &changeID
	ch.Changesets = append(ch.Changesets, changeID)
	ch.UpdatedAt = time.Now().UTC()
}

// ChangesetsSince returns changeset IDs after the given one (exclusive).
// If sinceID is nil, returns all changesets.
func (ch *Channel) ChangesetsSince(sinceID *string) []string {
	if sinceID == nil {
		result := make([]string, len(ch.Changesets))
		copy(result, ch.Changesets)
		return result
	}
	for i, id := range ch.Changesets {
		if id == *sinceID {
			result := make([]string, len(ch.Changesets)-i-1)
			copy(result, ch.Changesets[i+1:])
			return result
		}
	}
	// sinceID not found — return all
	result := make([]string, len(ch.Changesets))
	copy(result, ch.Changesets)
	return result
}

// ---------------------------------------------------------------------------
// Repository configuration
// ---------------------------------------------------------------------------

// UserConfig holds user identity.
type UserConfig struct {
	Name  string `json:"name"`
	Email string `json:"email"`
}

// RepoConfig holds repository-level configuration.
type RepoConfig struct {
	RemoteURL *string    `json:"remote_url"`
	User      UserConfig `json:"user"`
}

// DefaultRepoConfig returns a RepoConfig with sensible defaults.
func DefaultRepoConfig() RepoConfig {
	return RepoConfig{
		RemoteURL: nil,
		User: UserConfig{
			Name:  "unknown",
			Email: "unknown@example.com",
		},
	}
}

// ---------------------------------------------------------------------------
// Staging area
// ---------------------------------------------------------------------------

// StagedChange represents a staged change ready to be committed.
type StagedChange struct {
	ResourceID string           `json:"resource_id"`
	FilePath   string           `json:"file_path"`
	Previous   json.RawMessage  `json:"previous"`  // null if new
	Current    json.RawMessage  `json:"current"`
	Operations []PatchOperation `json:"operations"`
}

// ---------------------------------------------------------------------------
// Conflict
// ---------------------------------------------------------------------------

// Conflict represents a merge conflict on a specific JSON path.
type Conflict struct {
	ResourceID  string          `json:"resource_id"`
	JSONPath    string          `json:"json_path"`
	LocalValue  json.RawMessage `json:"local_value"`
	RemoteValue json.RawMessage `json:"remote_value"`
	BaseValue   json.RawMessage `json:"base_value"`
}

// ---------------------------------------------------------------------------
// Sync state
// ---------------------------------------------------------------------------

// SyncState tracks synchronisation progress with the remote server.
type SyncState struct {
	RemoteHeads      map[string]string `json:"remote_heads"`
	PushedChangesets []string          `json:"pushed_changesets"`
}

// DefaultSyncState returns a zero-value SyncState.
func DefaultSyncState() SyncState {
	return SyncState{
		RemoteHeads:      map[string]string{},
		PushedChangesets: []string{},
	}
}

// ---------------------------------------------------------------------------
// Hash utilities
// ---------------------------------------------------------------------------

// ContentHash returns "sha256:<hex>" for the given bytes.
func ContentHash(data []byte) string {
	h := sha256.Sum256(data)
	return "sha256:" + hex.EncodeToString(h[:])
}

// SHA256Hex returns the hex-encoded SHA-256 of the given bytes.
func SHA256Hex(data []byte) string {
	h := sha256.Sum256(data)
	return hex.EncodeToString(h[:])
}

// GenerateChangeID returns a 16-character random hex string.
func GenerateChangeID() string {
	b := make([]byte, 8)
	if _, err := rand.Read(b); err != nil {
		panic(fmt.Sprintf("crypto/rand failed: %v", err))
	}
	return hex.EncodeToString(b)
}
