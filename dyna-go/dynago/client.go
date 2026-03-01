package dynago

import (
	"context"
	"encoding/json"
	"fmt"

	"github.com/spf13/afero"
)

// Client is the main entry point for Go applications integrating with Dyna.
// It wraps a Repository (local state on afero.Fs) and a SyncClient (remote
// communication). All methods are safe for single-goroutine use; callers must
// synchronise externally for concurrent access.
type Client struct {
	Repo *Repository
	sync *SyncClient
}

// NewClient creates a Client backed by the given afero.Fs.
func NewClient(fs afero.Fs, root string) *Client {
	return &Client{
		Repo: NewRepository(fs, root),
	}
}

// NewMemClient creates a Client backed by an in-memory filesystem.
func NewMemClient() *Client {
	return &Client{
		Repo: NewMemRepository(),
	}
}

// ---------------------------------------------------------------------------
// Initialisation
// ---------------------------------------------------------------------------

// Init initialises a new local repository with an optional remote URL.
func (c *Client) Init(remoteURL *string) error {
	if err := c.Repo.Init(); err != nil {
		return err
	}
	if remoteURL != nil {
		cfg, _ := c.Repo.LoadConfig()
		cfg.RemoteURL = remoteURL
		if err := c.Repo.SaveConfig(cfg); err != nil {
			return err
		}
		c.sync = NewSyncClient(*remoteURL)
	}
	return nil
}

// IsInitialized returns true if the repository has been initialised.
func (c *Client) IsInitialized() bool {
	return c.Repo.IsInitialized()
}

// ensureSync lazily creates the SyncClient from config.
func (c *Client) ensureSync() error {
	if c.sync != nil {
		return nil
	}
	cfg, err := c.Repo.LoadConfig()
	if err != nil {
		return err
	}
	if cfg.RemoteURL == nil {
		return fmt.Errorf("no remote URL configured; use SetRemote first")
	}
	c.sync = NewSyncClient(*cfg.RemoteURL)
	return nil
}

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

// RemoteURL returns the configured remote URL.
func (c *Client) RemoteURL() (*string, error) {
	cfg, err := c.Repo.LoadConfig()
	if err != nil {
		return nil, err
	}
	return cfg.RemoteURL, nil
}

// SetRemote sets the remote server URL.
func (c *Client) SetRemote(url string) error {
	cfg, err := c.Repo.LoadConfig()
	if err != nil {
		return err
	}
	cfg.RemoteURL = &url
	c.sync = NewSyncClient(url)
	return c.Repo.SaveConfig(cfg)
}

// UserName returns the configured user name.
func (c *Client) UserName() (string, error) {
	cfg, err := c.Repo.LoadConfig()
	if err != nil {
		return "", err
	}
	return cfg.User.Name, nil
}

// SetUserName sets the user name.
func (c *Client) SetUserName(name string) error {
	cfg, err := c.Repo.LoadConfig()
	if err != nil {
		return err
	}
	cfg.User.Name = name
	return c.Repo.SaveConfig(cfg)
}

// ---------------------------------------------------------------------------
// Resource I/O (delegated to Repository)
// ---------------------------------------------------------------------------

// WriteResource writes a JSON resource to the working directory.
func (c *Client) WriteResource(resourceID string, value json.RawMessage) error {
	return c.Repo.WriteResource(resourceID, value)
}

// ReadResource reads a JSON resource from the working directory.
func (c *Client) ReadResource(resourceID string) (json.RawMessage, error) {
	return c.Repo.ReadResource(resourceID)
}

// DeleteResource removes a resource from the working directory.
func (c *Client) DeleteResource(resourceID string) error {
	return c.Repo.DeleteResource(resourceID)
}

// ListResources returns all resource IDs in the working directory.
func (c *Client) ListResources() ([]string, error) {
	return c.Repo.ListResources()
}

// ResourceExists checks if a resource exists.
func (c *Client) ResourceExists(resourceID string) bool {
	return c.Repo.ResourceExists(resourceID)
}

// ---------------------------------------------------------------------------
// Staging (delegated to Repository)
// ---------------------------------------------------------------------------

// Add stages a file for the next commit.
func (c *Client) Add(relativePath string) error {
	return c.Repo.Add(relativePath)
}

// AddByResourceID stages a resource by its ID.
func (c *Client) AddByResourceID(resourceID string) error {
	return c.Repo.Add(PathForResourceID(resourceID))
}

// AddDelete stages a file deletion.
func (c *Client) AddDelete(relativePath string) error {
	return c.Repo.AddDelete(relativePath)
}

// ---------------------------------------------------------------------------
// Local operations (delegated to Repository)
// ---------------------------------------------------------------------------

// Commit creates a changeset from staged changes.
func (c *Client) Commit(message string) (string, error) {
	return c.Repo.Commit(message)
}

// Status returns the current repository status.
func (c *Client) Status() (*StatusResult, error) {
	return c.Repo.Status()
}

// Log returns the changeset history for the current channel.
func (c *Client) Log() ([]LogEntry, error) {
	return c.Repo.Log()
}

// Diff returns the patches for a changeset (or staged changes if nil).
func (c *Client) Diff(changeID *string) (*DiffResult, error) {
	return c.Repo.DiffChangeset(changeID)
}

// Restore restores a working file from the snapshot.
func (c *Client) Restore(relativePath string) error {
	return c.Repo.Restore(relativePath)
}

// Describe amends the message of the latest changeset.
func (c *Client) Describe(newMessage string) (string, error) {
	return c.Repo.Describe(newMessage)
}

// Squash combines mutable changesets into one.
func (c *Client) Squash() (string, error) {
	return c.Repo.Squash()
}

// Revert creates an inverse changeset.
func (c *Client) Revert(changeID string) (string, error) {
	return c.Repo.Revert(changeID)
}

// CherryPick copies a changeset from another channel.
func (c *Client) CherryPick(changeID string) (string, error) {
	return c.Repo.CherryPick(changeID)
}

// Resolve marks a conflicted resource as resolved.
func (c *Client) Resolve(resourceID string) error {
	return c.Repo.Resolve(resourceID)
}

// ListConflicts returns resource IDs with unresolved conflicts.
func (c *Client) ListConflicts() ([]string, error) {
	return c.Repo.ListConflictedResources()
}

// ---------------------------------------------------------------------------
// Channel operations (delegated to Repository)
// ---------------------------------------------------------------------------

// CurrentChannel returns the name of the active channel.
func (c *Client) CurrentChannel() (string, error) {
	return c.Repo.CurrentChannelName()
}

// ListChannels returns all local channels.
func (c *Client) ListChannels() ([]Channel, error) {
	return c.Repo.ListChannels()
}

// CreateChannel creates a new local channel.
func (c *Client) CreateChannel(name string, forkFrom *string) error {
	return c.Repo.CreateChannel(name, forkFrom)
}

// SwitchChannel changes the active channel.
func (c *Client) SwitchChannel(name string) error {
	return c.Repo.SwitchChannel(name)
}

// ---------------------------------------------------------------------------
// Snapshot access
// ---------------------------------------------------------------------------

// GetSnapshot returns the current snapshot for a resource.
func (c *Client) GetSnapshot(resourceID string) (json.RawMessage, error) {
	return c.Repo.LoadSnapshot(resourceID)
}

// ListSnapshots returns all resource IDs with snapshots.
func (c *Client) ListSnapshots() ([]string, error) {
	return c.Repo.ListSnapshots()
}

// GetChangeset returns a changeset by its change_id (prefix match).
func (c *Client) GetChangeset(changeID string) (*Changeset, error) {
	return c.Repo.FindChangesetByPrefix(changeID)
}

// ---------------------------------------------------------------------------
// Remote operations
// ---------------------------------------------------------------------------

// Push sends local changesets to the remote server.
func (c *Client) Push(ctx context.Context) (*PushResponse, error) {
	if err := c.ensureSync(); err != nil {
		return nil, err
	}

	channelName, err := c.Repo.CurrentChannelName()
	if err != nil {
		return nil, err
	}
	if channelName == "main" {
		return nil, fmt.Errorf("cannot push directly to the 'main' channel; promote instead")
	}

	ch, err := c.Repo.LoadChannel(channelName)
	if err != nil {
		return nil, err
	}

	ss, err := c.Repo.LoadSyncState()
	if err != nil {
		return nil, err
	}

	// Determine which changesets haven't been pushed yet
	pushedSet := map[string]bool{}
	for _, id := range ss.PushedChangesets {
		pushedSet[id] = true
	}

	var toPush []Changeset
	for _, cid := range ch.Changesets {
		if !pushedSet[cid] {
			cs, err := c.Repo.LoadChangeset(cid)
			if err != nil {
				continue
			}
			toPush = append(toPush, *cs)
		}
	}

	if len(toPush) == 0 {
		return &PushResponse{Success: true}, nil
	}

	resp, err := c.sync.Push(ctx, &PushRequest{
		Channel:    channelName,
		Changesets: toPush,
	})
	if err != nil {
		return nil, err
	}

	// Update sync state
	for _, cs := range toPush {
		ss.PushedChangesets = append(ss.PushedChangesets, cs.ChangeID)
		// Mark as immutable
		cs.Immutable = true
		_ = c.Repo.SaveChangeset(&cs)
	}
	if resp.NewHead != nil {
		ss.RemoteHeads[channelName] = *resp.NewHead
	}
	_ = c.Repo.SaveSyncState(ss)

	return resp, nil
}

// Pull fetches new changesets from the remote server.
func (c *Client) Pull(ctx context.Context) (*PullResponse, error) {
	if err := c.ensureSync(); err != nil {
		return nil, err
	}

	channelName, err := c.Repo.CurrentChannelName()
	if err != nil {
		return nil, err
	}

	ch, err := c.Repo.LoadChannel(channelName)
	if err != nil {
		return nil, err
	}

	resp, err := c.sync.Pull(ctx, &PullRequest{
		Channel:       channelName,
		SinceChangeID: ch.HeadChangeID,
	})
	if err != nil {
		return nil, err
	}

	// Apply pulled changesets — smart dedup: skip if already local
	for _, cs := range resp.Changesets {
		cs.Immutable = true

		// Check if changeset already exists locally (e.g. from another channel)
		existing, _ := c.Repo.LoadChangeset(cs.ChangeID)
		if existing == nil {
			// New changeset — save it
			if err := c.Repo.SaveChangeset(&cs); err != nil {
				continue
			}
		}

		// Import into this channel regardless
		ch.AppendChangeset(cs.ChangeID)

		// Apply patches to per-channel snapshots
		for _, p := range cs.Patches {
			if p.ResultSnapshot != nil {
				_ = c.Repo.SaveSnapshot(p.TargetResource, p.ResultSnapshot)
				_ = c.Repo.WriteWorkFile(PathForResourceID(p.TargetResource), p.ResultSnapshot)
			} else {
				snap, _ := c.Repo.LoadSnapshot(p.TargetResource)
				if snap == nil {
					snap = json.RawMessage("{}")
				}
				if err := ApplyPatch(&snap, p.Operations); err == nil {
					_ = c.Repo.SaveSnapshot(p.TargetResource, snap)
					_ = c.Repo.WriteWorkFile(PathForResourceID(p.TargetResource), snap)
				}
			}
		}
	}

	if err := c.Repo.SaveChannel(ch); err != nil {
		return nil, err
	}

	// Update sync state
	ss, _ := c.Repo.LoadSyncState()
	if resp.CurrentHead != nil {
		ss.RemoteHeads[channelName] = *resp.CurrentHead
	}
	for _, cs := range resp.Changesets {
		ss.PushedChangesets = append(ss.PushedChangesets, cs.ChangeID)
	}
	_ = c.Repo.SaveSyncState(ss)

	return resp, nil
}

// CloneRepo clones a repository from the remote server.
func (c *Client) CloneRepo(ctx context.Context, remoteURL string) error {
	c.sync = NewSyncClient(remoteURL)

	resp, err := c.sync.CloneRepo(ctx, &CloneRequest{})
	if err != nil {
		return err
	}

	// Initialise local repo
	if err := c.Repo.Init(); err != nil {
		return err
	}

	// Set remote
	cfg, _ := c.Repo.LoadConfig()
	cfg.RemoteURL = &remoteURL
	_ = c.Repo.SaveConfig(cfg)

	// Save channels
	for _, ch := range resp.Channels {
		chCopy := ch
		_ = c.Repo.SaveChannel(&chCopy)
	}

	// Save changesets
	for i := range resp.Changesets {
		resp.Changesets[i].Immutable = true
		_ = c.Repo.SaveChangeset(&resp.Changesets[i])
	}

	// Save snapshots and working files
	for resourceID, snap := range resp.Snapshots {
		_ = c.Repo.SaveSnapshot(resourceID, snap)
		_ = c.Repo.WriteWorkFile(PathForResourceID(resourceID), snap)
	}

	// Update sync state
	ss := DefaultSyncState()
	for _, ch := range resp.Channels {
		if ch.HeadChangeID != nil {
			ss.RemoteHeads[ch.Name] = *ch.HeadChangeID
		}
	}
	for _, cs := range resp.Changesets {
		ss.PushedChangesets = append(ss.PushedChangesets, cs.ChangeID)
	}
	_ = c.Repo.SaveSyncState(&ss)

	return nil
}

// Promote promotes changesets from one channel to another on the remote,
// then applies locally if successful.
func (c *Client) Promote(ctx context.Context, sourceChannel, targetChannel string) (*PromoteResponse, error) {
	if err := c.ensureSync(); err != nil {
		return nil, err
	}

	// Remote-first: promote on server
	resp, err := c.sync.Promote(ctx, &PromoteRequest{
		SourceChannel: sourceChannel,
		TargetChannel: targetChannel,
	})
	if err != nil {
		return nil, err
	}

	// Apply promoted changesets locally
	targetCh, err := c.Repo.LoadChannel(targetChannel)
	if err != nil {
		// Create target channel locally if it doesn't exist
		newCh := NewChannel(targetChannel)
		targetCh = &newCh
	}

	for _, cs := range resp.PromotedChangesets {
		csCopy := cs
		csCopy.Immutable = true
		_ = c.Repo.SaveChangeset(&csCopy)
		targetCh.AppendChangeset(cs.ChangeID)

		// Apply patches to snapshots
		for _, p := range cs.Patches {
			snap, _ := c.Repo.LoadSnapshot(p.TargetResource)
			if snap == nil {
				snap = json.RawMessage("{}")
			}
			if err := ApplyPatch(&snap, p.Operations); err == nil {
				_ = c.Repo.SaveSnapshot(p.TargetResource, snap)
			}
		}
	}

	_ = c.Repo.SaveChannel(targetCh)

	return resp, nil
}

// ListRemoteChannels fetches channels from the remote server.
func (c *Client) ListRemoteChannels(ctx context.Context) ([]Channel, error) {
	if err := c.ensureSync(); err != nil {
		return nil, err
	}
	resp, err := c.sync.ListRemoteChannels(ctx)
	if err != nil {
		return nil, err
	}
	return resp.Channels, nil
}

// Health checks the remote server health.
func (c *Client) Health(ctx context.Context) (*HealthResponse, error) {
	if err := c.ensureSync(); err != nil {
		return nil, err
	}
	return c.sync.Health(ctx)
}

// ResourceHistory queries the change history of a resource on the remote.
func (c *Client) ResourceHistory(ctx context.Context, resourceID string) (*ResourceHistoryResponse, error) {
	if err := c.ensureSync(); err != nil {
		return nil, err
	}
	return c.sync.ResourceHistory(ctx, resourceID)
}

// SubscribeNotifications connects to the server's WebSocket notification stream.
func (c *Client) SubscribeNotifications(ctx context.Context, handler NotificationHandler) error {
	if err := c.ensureSync(); err != nil {
		return err
	}
	return c.sync.SubscribeNotifications(ctx, handler)
}
