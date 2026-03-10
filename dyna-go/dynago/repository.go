package dynago

import (
	"encoding/json"
	"fmt"
	"io/fs"
	"path/filepath"
	"sort"
	"strings"
	"github.com/spf13/afero"
)

// Repository manages all local state for a Dyna repository using an afero.Fs.
// The filesystem layout mirrors the Rust implementation:
//
//	<root>/
//	  .dyna/
//	    config.json
//	    current_channel       (plain text)
//	    staged.json
//	    sync_state.json
//	    channels/<name>.json
//	    changesets/<change_id>.json
//	    snapshots/<resource_id>.json
//	    conflicts/<resource_id>.json
//	  <working files: resource_id mapped to path>
type Repository struct {
	fs      afero.Fs
	root    string // root directory path (e.g. "/" for MemMapFs)
	dynaDir string // root + "/.dyna"
}

// NewRepository creates a Repository backed by the given afero.Fs.
// root is the base directory (use "/" for a MemMapFs).
func NewRepository(fs afero.Fs, root string) *Repository {
	return &Repository{
		fs:      fs,
		root:    root,
		dynaDir: filepath.Join(root, ".dyna"),
	}
}

// NewMemRepository creates a Repository backed by an in-memory filesystem.
func NewMemRepository() *Repository {
	return NewRepository(afero.NewMemMapFs(), "/")
}

// Init initialises the repository directory structure and default config.
func (r *Repository) Init() error {
	exists, err := afero.DirExists(r.fs, r.dynaDir)
	if err != nil {
		return err
	}
	if exists {
		return fmt.Errorf("repository already initialized at %s", r.root)
	}

	for _, dir := range []string{
		r.dynaDir,
		filepath.Join(r.dynaDir, "channels"),
		filepath.Join(r.dynaDir, "changesets"),
		filepath.Join(r.dynaDir, "snapshots"),
		filepath.Join(r.dynaDir, "snapshots", "main"),
		filepath.Join(r.dynaDir, "conflicts"),
	} {
		if err := r.fs.MkdirAll(dir, 0755); err != nil {
			return err
		}
	}

	// Default config
	cfg := DefaultRepoConfig()
	if err := r.SaveConfig(&cfg); err != nil {
		return err
	}

	// Create "main" channel
	main := NewChannel("main")
	if err := r.SaveChannel(&main); err != nil {
		return err
	}

	// Set current channel
	if err := r.setCurrentChannelName("main"); err != nil {
		return err
	}

	// Empty staging area
	if err := r.SaveStaged(nil); err != nil {
		return err
	}

	// Default sync state
	ss := DefaultSyncState()
	return r.SaveSyncState(&ss)
}

// IsInitialized returns true if the .dyna directory exists.
func (r *Repository) IsInitialized() bool {
	ok, _ := afero.DirExists(r.fs, r.dynaDir)
	return ok
}

// Fs returns the underlying afero.Fs.
func (r *Repository) Fs() afero.Fs { return r.fs }

// Root returns the repository root path.
func (r *Repository) Root() string { return r.root }

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

func (r *Repository) configPath() string {
	return filepath.Join(r.dynaDir, "config.json")
}

// LoadConfig reads the repository configuration.
func (r *Repository) LoadConfig() (*RepoConfig, error) {
	var cfg RepoConfig
	if err := r.readJSON(r.configPath(), &cfg); err != nil {
		return nil, err
	}
	return &cfg, nil
}

// SaveConfig writes the repository configuration.
func (r *Repository) SaveConfig(cfg *RepoConfig) error {
	return r.writeJSON(r.configPath(), cfg)
}

// ---------------------------------------------------------------------------
// Current channel
// ---------------------------------------------------------------------------

func (r *Repository) currentChannelPath() string {
	return filepath.Join(r.dynaDir, "current_channel")
}

// CurrentChannelName returns the name of the active channel.
func (r *Repository) CurrentChannelName() (string, error) {
	data, err := afero.ReadFile(r.fs, r.currentChannelPath())
	if err != nil {
		return "", err
	}
	return strings.TrimSpace(string(data)), nil
}

func (r *Repository) setCurrentChannelName(name string) error {
	return afero.WriteFile(r.fs, r.currentChannelPath(), []byte(name), 0644)
}

// CurrentChannel loads the active Channel struct.
func (r *Repository) CurrentChannel() (*Channel, error) {
	name, err := r.CurrentChannelName()
	if err != nil {
		return nil, err
	}
	return r.LoadChannel(name)
}

// ---------------------------------------------------------------------------
// Channels
// ---------------------------------------------------------------------------

func (r *Repository) channelPath(name string) string {
	return filepath.Join(r.dynaDir, "channels", name+".json")
}

// LoadChannel reads a channel by name.
func (r *Repository) LoadChannel(name string) (*Channel, error) {
	var ch Channel
	if err := r.readJSON(r.channelPath(name), &ch); err != nil {
		return nil, fmt.Errorf("channel not found: %s", name)
	}
	return &ch, nil
}

// SaveChannel persists a channel.
func (r *Repository) SaveChannel(ch *Channel) error {
	return r.writeJSON(r.channelPath(ch.Name), ch)
}

// ListChannels returns all channels.
func (r *Repository) ListChannels() ([]Channel, error) {
	names, err := r.listJSONStems(filepath.Join(r.dynaDir, "channels"))
	if err != nil {
		return nil, err
	}
	channels := make([]Channel, 0, len(names))
	for _, name := range names {
		ch, err := r.LoadChannel(name)
		if err != nil {
			continue
		}
		channels = append(channels, *ch)
	}
	return channels, nil
}

// CreateChannel creates a new channel, optionally forking from another.
func (r *Repository) CreateChannel(name string, forkFrom *string) error {
	if _, err := r.LoadChannel(name); err == nil {
		return fmt.Errorf("channel already exists: %s", name)
	}

	ch := NewChannel(name)
	if forkFrom != nil {
		src, err := r.LoadChannel(*forkFrom)
		if err != nil {
			return err
		}
		ch.Changesets = make([]string, len(src.Changesets))
		copy(ch.Changesets, src.Changesets)
		ch.HeadChangeID = src.HeadChangeID

		// Copy snapshots from source channel to the new channel
		srcDir := r.snapshotDirFor(*forkFrom)
		dstDir := r.snapshotDirFor(name)
		entries, _ := afero.ReadDir(r.fs, srcDir)
		for _, entry := range entries {
			if entry.IsDir() {
				continue
			}
			data, err := afero.ReadFile(r.fs, filepath.Join(srcDir, entry.Name()))
			if err != nil {
				continue
			}
			_ = afero.WriteFile(r.fs, filepath.Join(dstDir, entry.Name()), data, 0644)
		}
	}
	return r.SaveChannel(&ch)
}

// SwitchChannel changes the active channel and rebuilds the working directory
// from the new channel's snapshots.
func (r *Repository) SwitchChannel(name string) error {
	if _, err := r.LoadChannel(name); err != nil {
		return err
	}

	// Clear existing working directory JSON files
	oldFiles, _ := r.ListWorkJSONFiles()
	for _, f := range oldFiles {
		_ = r.RemoveWorkFile(f)
	}

	// Switch the current channel
	if err := r.setCurrentChannelName(name); err != nil {
		return err
	}

	// Rebuild working directory from the new channel's snapshots
	allSnaps, _ := r.LoadAllSnapshots()
	for resourceID, snap := range allSnaps {
		_ = r.WriteWorkFile(PathForResourceID(resourceID), snap)
	}

	return nil
}

// DeleteChannel removes a channel's JSON file and its snapshot directory.
func (r *Repository) DeleteChannel(name string) error {
	channelPath := filepath.Join(r.dynaDir, "channels", name+".json")
	_ = r.fs.Remove(channelPath)
	// Remove snapshot directory
	snapDir := r.snapshotDirFor(name)
	entries, _ := afero.ReadDir(r.fs, snapDir)
	for _, entry := range entries {
		_ = r.fs.Remove(filepath.Join(snapDir, entry.Name()))
	}
	_ = r.fs.Remove(snapDir)
	return nil
}

// ---------------------------------------------------------------------------
// Changesets
// ---------------------------------------------------------------------------

func (r *Repository) changesetPath(changeID string) string {
	return filepath.Join(r.dynaDir, "changesets", changeID+".json")
}

// LoadChangeset reads a changeset by its change_id.
func (r *Repository) LoadChangeset(changeID string) (*Changeset, error) {
	var cs Changeset
	if err := r.readJSON(r.changesetPath(changeID), &cs); err != nil {
		return nil, fmt.Errorf("changeset not found: %s", changeID)
	}
	return &cs, nil
}

// SaveChangeset persists a changeset.
func (r *Repository) SaveChangeset(cs *Changeset) error {
	return r.writeJSON(r.changesetPath(cs.ChangeID), cs)
}

// FindChangesetByPrefix resolves a changeset by prefix match.
func (r *Repository) FindChangesetByPrefix(prefix string) (*Changeset, error) {
	names, err := r.listJSONStems(filepath.Join(r.dynaDir, "changesets"))
	if err != nil {
		return nil, err
	}
	var matches []string
	for _, name := range names {
		if strings.HasPrefix(name, prefix) {
			matches = append(matches, name)
		}
	}
	switch len(matches) {
	case 0:
		return nil, fmt.Errorf("changeset not found: %s", prefix)
	case 1:
		return r.LoadChangeset(matches[0])
	default:
		return nil, fmt.Errorf("ambiguous changeset prefix '%s': matches %v", prefix, matches)
	}
}

// ---------------------------------------------------------------------------
// Snapshots
// ---------------------------------------------------------------------------

// snapshotDir returns the snapshot directory for the current channel.
func (r *Repository) snapshotDir() (string, error) {
	name, err := r.CurrentChannelName()
	if err != nil {
		return "", err
	}
	dir := filepath.Join(r.dynaDir, "snapshots", name)
	_ = r.fs.MkdirAll(dir, 0755)
	return dir, nil
}

// snapshotDirFor returns the snapshot directory for a specific channel.
func (r *Repository) snapshotDirFor(channel string) string {
	dir := filepath.Join(r.dynaDir, "snapshots", channel)
	_ = r.fs.MkdirAll(dir, 0755)
	return dir
}

func (r *Repository) snapshotPath(resourceID string) (string, error) {
	dir, err := r.snapshotDir()
	if err != nil {
		return "", err
	}
	return filepath.Join(dir, resourceID+".json"), nil
}

// LoadSnapshot reads the current snapshot for a resource in the current channel.
// Returns nil, nil if the snapshot does not exist.
func (r *Repository) LoadSnapshot(resourceID string) (json.RawMessage, error) {
	path, err := r.snapshotPath(resourceID)
	if err != nil {
		return nil, err
	}
	data, err := afero.ReadFile(r.fs, path)
	if err != nil {
		return nil, nil // not found is not an error
	}
	return json.RawMessage(data), nil
}

// SaveSnapshot persists a resource snapshot in the current channel.
func (r *Repository) SaveSnapshot(resourceID string, value json.RawMessage) error {
	path, err := r.snapshotPath(resourceID)
	if err != nil {
		return err
	}
	if err := r.fs.MkdirAll(filepath.Dir(path), 0755); err != nil {
		return err
	}
	return afero.WriteFile(r.fs, path, value, 0644)
}

// SaveSnapshotForChannel persists a resource snapshot in a specific channel.
func (r *Repository) SaveSnapshotForChannel(channel, resourceID string, value json.RawMessage) error {
	dir := r.snapshotDirFor(channel)
	path := filepath.Join(dir, resourceID+".json")
	return afero.WriteFile(r.fs, path, value, 0644)
}

// RemoveSnapshot deletes a resource snapshot in the current channel.
func (r *Repository) RemoveSnapshot(resourceID string) error {
	path, err := r.snapshotPath(resourceID)
	if err != nil {
		return err
	}
	exists, _ := afero.Exists(r.fs, path)
	if exists {
		return r.fs.Remove(path)
	}
	return nil
}

// ListSnapshots returns all resource IDs that have snapshots in the current channel.
func (r *Repository) ListSnapshots() ([]string, error) {
	dir, err := r.snapshotDir()
	if err != nil {
		return nil, err
	}
	return r.listJSONStems(dir)
}

// LoadAllSnapshots returns all snapshots as a map for the current channel.
func (r *Repository) LoadAllSnapshots() (map[string]json.RawMessage, error) {
	ids, err := r.ListSnapshots()
	if err != nil {
		return nil, err
	}
	result := make(map[string]json.RawMessage, len(ids))
	for _, id := range ids {
		snap, err := r.LoadSnapshot(id)
		if err != nil {
			return nil, err
		}
		if snap != nil {
			result[id] = snap
		}
	}
	return result, nil
}

// ---------------------------------------------------------------------------
// Staging area
// ---------------------------------------------------------------------------

func (r *Repository) stagedPath() string {
	return filepath.Join(r.dynaDir, "staged.json")
}

// LoadStaged reads the current staging area.
func (r *Repository) LoadStaged() ([]StagedChange, error) {
	var staged []StagedChange
	if err := r.readJSON(r.stagedPath(), &staged); err != nil {
		// If file doesn't exist, return empty
		return nil, nil
	}
	return staged, nil
}

// SaveStaged persists the staging area.
func (r *Repository) SaveStaged(staged []StagedChange) error {
	if staged == nil {
		staged = []StagedChange{}
	}
	return r.writeJSON(r.stagedPath(), staged)
}

// ---------------------------------------------------------------------------
// Sync state
// ---------------------------------------------------------------------------

func (r *Repository) syncStatePath() string {
	return filepath.Join(r.dynaDir, "sync_state.json")
}

// LoadSyncState reads the sync state.
func (r *Repository) LoadSyncState() (*SyncState, error) {
	var ss SyncState
	if err := r.readJSON(r.syncStatePath(), &ss); err != nil {
		def := DefaultSyncState()
		return &def, nil
	}
	if ss.RemoteHeads == nil {
		ss.RemoteHeads = map[string]string{}
	}
	return &ss, nil
}

// SaveSyncState persists the sync state.
func (r *Repository) SaveSyncState(ss *SyncState) error {
	return r.writeJSON(r.syncStatePath(), ss)
}

// ---------------------------------------------------------------------------
// Conflicts
// ---------------------------------------------------------------------------

func (r *Repository) conflictPath(resourceID string) string {
	return filepath.Join(r.dynaDir, "conflicts", resourceID+".json")
}

// LoadConflicts reads conflicts for a resource.
func (r *Repository) LoadConflicts(resourceID string) ([]Conflict, error) {
	var conflicts []Conflict
	if err := r.readJSON(r.conflictPath(resourceID), &conflicts); err != nil {
		return nil, nil
	}
	return conflicts, nil
}

// SaveConflicts persists conflicts for a resource.
func (r *Repository) SaveConflicts(resourceID string, conflicts []Conflict) error {
	path := r.conflictPath(resourceID)
	if err := r.fs.MkdirAll(filepath.Dir(path), 0755); err != nil {
		return err
	}
	return r.writeJSON(path, conflicts)
}

// ClearConflicts removes conflict data for a resource.
func (r *Repository) ClearConflicts(resourceID string) error {
	path := r.conflictPath(resourceID)
	exists, _ := afero.Exists(r.fs, path)
	if exists {
		return r.fs.Remove(path)
	}
	return nil
}

// ListConflictedResources returns resource IDs that have conflicts.
func (r *Repository) ListConflictedResources() ([]string, error) {
	return r.listJSONStems(filepath.Join(r.dynaDir, "conflicts"))
}

// ---------------------------------------------------------------------------
// Working directory (resource files)
// ---------------------------------------------------------------------------

// ResourceIDFromPath converts a relative file path to a resource ID.
// e.g. "acme/entity/User.json" → "acme.entity.User"
func ResourceIDFromPath(relativePath string) string {
	s := strings.TrimSuffix(relativePath, ".json")
	return strings.ReplaceAll(s, "/", ".")
}

// PathForResourceID converts a resource ID to a relative file path.
// e.g. "acme.entity.User" → "acme/entity/User.json"
func PathForResourceID(resourceID string) string {
	return strings.ReplaceAll(resourceID, ".", "/") + ".json"
}

// ReadWorkFile reads a file from the working directory.
func (r *Repository) ReadWorkFile(relativePath string) ([]byte, error) {
	return afero.ReadFile(r.fs, filepath.Join(r.root, relativePath))
}

// WriteWorkFile writes a file to the working directory.
func (r *Repository) WriteWorkFile(relativePath string, data []byte) error {
	fullPath := filepath.Join(r.root, relativePath)
	if err := r.fs.MkdirAll(filepath.Dir(fullPath), 0755); err != nil {
		return err
	}
	return afero.WriteFile(r.fs, fullPath, data, 0644)
}

// WorkFileExists checks if a working file exists.
func (r *Repository) WorkFileExists(relativePath string) bool {
	exists, _ := afero.Exists(r.fs, filepath.Join(r.root, relativePath))
	return exists
}

// RemoveWorkFile removes a file from the working directory.
func (r *Repository) RemoveWorkFile(relativePath string) error {
	path := filepath.Join(r.root, relativePath)
	exists, _ := afero.Exists(r.fs, path)
	if exists {
		return r.fs.Remove(path)
	}
	return nil
}

// ListWorkJSONFiles returns all .json file paths under the working directory,
// excluding the .dyna directory.
func (r *Repository) ListWorkJSONFiles() ([]string, error) {
	var results []string
	err := afero.Walk(r.fs, r.root, func(path string, info fs.FileInfo, err error) error {
		if err != nil {
			return nil // skip errors
		}
		// Skip .dyna directory
		rel, _ := filepath.Rel(r.root, path)
		if strings.HasPrefix(rel, ".dyna") {
			if info.IsDir() {
				return filepath.SkipDir
			}
			return nil
		}
		if !info.IsDir() && strings.HasSuffix(path, ".json") {
			results = append(results, rel)
		}
		return nil
	})
	if err != nil {
		return nil, err
	}
	sort.Strings(results)
	return results, nil
}

// ReadResource reads a resource by its ID from the working directory.
func (r *Repository) ReadResource(resourceID string) (json.RawMessage, error) {
	data, err := r.ReadWorkFile(PathForResourceID(resourceID))
	if err != nil {
		return nil, fmt.Errorf("resource not found: %s", resourceID)
	}
	return json.RawMessage(data), nil
}

// WriteResource writes a resource by its ID to the working directory.
func (r *Repository) WriteResource(resourceID string, value json.RawMessage) error {
	return r.WriteWorkFile(PathForResourceID(resourceID), value)
}

// DeleteResource removes a resource file from the working directory.
func (r *Repository) DeleteResource(resourceID string) error {
	return r.RemoveWorkFile(PathForResourceID(resourceID))
}

// ResourceExists checks if a resource file exists.
func (r *Repository) ResourceExists(resourceID string) bool {
	return r.WorkFileExists(PathForResourceID(resourceID))
}

// ListResources returns all resource IDs in the working directory.
func (r *Repository) ListResources() ([]string, error) {
	files, err := r.ListWorkJSONFiles()
	if err != nil {
		return nil, err
	}
	ids := make([]string, len(files))
	for i, f := range files {
		ids[i] = ResourceIDFromPath(f)
	}
	return ids, nil
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

func (r *Repository) readJSON(path string, v interface{}) error {
	data, err := afero.ReadFile(r.fs, path)
	if err != nil {
		return err
	}
	return json.Unmarshal(data, v)
}

func (r *Repository) writeJSON(path string, v interface{}) error {
	data, err := json.MarshalIndent(v, "", "  ")
	if err != nil {
		return err
	}
	if err := r.fs.MkdirAll(filepath.Dir(path), 0755); err != nil {
		return err
	}
	return afero.WriteFile(r.fs, path, data, 0644)
}

func (r *Repository) listJSONStems(dir string) ([]string, error) {
	var stems []string
	err := afero.Walk(r.fs, dir, func(path string, info fs.FileInfo, err error) error {
		if err != nil {
			return nil // skip errors
		}
		if info.IsDir() {
			return nil
		}
		if strings.HasSuffix(info.Name(), ".json") {
			// Compute relative path from dir, strip .json suffix
			rel, _ := filepath.Rel(dir, path)
			stems = append(stems, strings.TrimSuffix(rel, ".json"))
		}
		return nil
	})
	if err != nil {
		return nil, nil // directory doesn't exist → empty
	}
	sort.Strings(stems)
	return stems, nil
}
