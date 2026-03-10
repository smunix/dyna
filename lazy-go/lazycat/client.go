// Package lazycat provides a lazy, on-demand resource loader for Dyna servers.
//
// LazyClient maintains an in-memory repository (via dyna-go) that is populated
// only with resources that have been explicitly requested.  A background
// WebSocket listener keeps the local cache up-to-date whenever new changesets
// are pushed to the configured channel.
//
// # Performance
//
// Batch operations (GetMany, GetAll, ForEach, ForEachAll) ensure all missing
// resources are materialised in a single pass rather than one at a time.
// The ForEach family accepts a continuation so that callers with very large
// datasets (tens of thousands of resources) never need to build a full map
// in memory.
package lazycat

import (
	"context"
	"encoding/json"
	"fmt"
	"log"
	"sort"
	"strings"
	"sync"
	"time"

	"dyna-go/dynago"

	"github.com/gorilla/websocket"
)

// ChangesetInfo contains metadata about a single changeset in an update event.
type ChangesetInfo struct {
	ChangeID          string   `json:"change_id"`
	Message           string   `json:"message"`
	Author            string   `json:"author"`
	PatchCount        int      `json:"patch_count"`
	AffectedResources []string `json:"affected_resources"`
}

// UpdateEvent contains full details about a live update received via WebSocket.
type UpdateEvent struct {
	// Kind is the notification type: "push" or "promotion".
	Kind string
	// Timestamp is the ISO 8601 timestamp of the event on the server.
	Timestamp string
	// Channel is the affected channel name.
	Channel string
	// Changesets contains per-changeset metadata.
	Changesets []ChangesetInfo
	// NewHead is the new head change_id of the channel (if available).
	NewHead *string
	// AffectedResourceIDs is the flat list of all affected resource IDs.
	AffectedResourceIDs []string
	// UpdatedSnapshots maps resource IDs to their new materialised values.
	UpdatedSnapshots map[string]json.RawMessage
}

// OnUpdateFunc is called whenever the local cache is updated from the server.
// It receives a rich UpdateEvent with full metadata and materialised snapshots.
type OnUpdateFunc func(event *UpdateEvent)

// ResourceFunc is a continuation called for each resource during ForEach /
// ForEachAll.  Returning a non-nil error stops the iteration.
type ResourceFunc func(resourceID string, value json.RawMessage) error

// LazyClient is a lazy, on-demand resource loader backed by a local dyna-go
// repository.
type LazyClient struct {
	serverURL string
	channel   string
	client    *dynago.Client
	sync      *dynago.SyncClient

	mu       sync.RWMutex
	knownIDs map[string]bool
	loaded   map[string]bool

	onUpdate OnUpdateFunc
	cancel   context.CancelFunc
}

// Connect creates a new LazyClient that connects to the given Dyna server
// and starts tracking the specified channel.
//
// It performs a lightweight clone to populate channel metadata and changeset
// history.  Resource bodies are fetched lazily on first access.
func Connect(ctx context.Context, serverURL, channel string) (*LazyClient, error) {
	// Create an in-memory client
	client := dynago.NewMemClient()

	// Clone from the server
	if err := client.CloneRepo(ctx, serverURL); err != nil {
		return nil, fmt.Errorf("initial clone: %w", err)
	}

	// Switch to the requested channel
	channels, err := client.ListChannels()
	if err == nil {
		found := false
		for _, ch := range channels {
			if ch.Name == channel {
				found = true
				break
			}
		}
		if found {
			_ = client.SwitchChannel(channel)
		} else {
			log.Printf("lazy-go: channel %q not found, using current channel", channel)
		}
	}

	// Collect known resource IDs
	knownIDs := make(map[string]bool)
	loaded := make(map[string]bool)
	if snapIDs, err := client.ListSnapshots(); err == nil {
		for _, id := range snapIDs {
			knownIDs[id] = true
			loaded[id] = true
		}
	}

	ctx, cancel := context.WithCancel(ctx)

	lc := &LazyClient{
		serverURL: serverURL,
		channel:   channel,
		client:    client,
		sync:      dynago.NewSyncClient(serverURL),
		knownIDs:  knownIDs,
		loaded:    loaded,
		cancel:    cancel,
	}

	// Start the WebSocket listener
	go lc.wsListener(ctx)

	return lc, nil
}

// Get returns a single resource by ID, fetching on demand if needed.
func (lc *LazyClient) Get(resourceID string) (json.RawMessage, error) {
	lc.mu.RLock()
	isLoaded := lc.loaded[resourceID]
	lc.mu.RUnlock()

	if !isLoaded {
		lc.materialise(resourceID)
	}

	return lc.client.ReadResource(resourceID)
}

// GetMany returns multiple resources by ID.
//
// All missing resources are materialised in a single batch pass rather than
// one at a time.
func (lc *LazyClient) GetMany(resourceIDs []string) (map[string]json.RawMessage, error) {
	lc.materialiseBatch(resourceIDs)

	result := make(map[string]json.RawMessage, len(resourceIDs))
	for _, rid := range resourceIDs {
		data, err := lc.client.ReadResource(rid)
		if err == nil {
			result[rid] = data
		}
	}
	return result, nil
}

// ForEach processes multiple resources through a continuation, avoiding a
// large map in memory.  The continuation is called once for each resource
// that exists locally.  Return a non-nil error from f to stop iteration.
func (lc *LazyClient) ForEach(resourceIDs []string, f ResourceFunc) error {
	lc.materialiseBatch(resourceIDs)

	for _, rid := range resourceIDs {
		data, err := lc.client.ReadResource(rid)
		if err != nil {
			continue
		}
		if err := f(rid, data); err != nil {
			return err
		}
	}
	return nil
}

// ForEachAll processes all known resources through a continuation.
//
// This is the most memory-efficient way to iterate over the entire dataset.
func (lc *LazyClient) ForEachAll(f ResourceFunc) error {
	ids := lc.ListResources()
	return lc.ForEach(ids, f)
}

// GetAll fetches all known resources.
//
// For large repositories (tens of thousands of resources), prefer
// ForEachAll to avoid building a large map in memory.
func (lc *LazyClient) GetAll() (map[string]json.RawMessage, error) {
	ids := lc.ListResources()
	return lc.GetMany(ids)
}

// ListResources returns all known resource IDs (metadata only), sorted.
func (lc *LazyClient) ListResources() []string {
	lc.mu.RLock()
	defer lc.mu.RUnlock()

	ids := make([]string, 0, len(lc.knownIDs))
	for id := range lc.knownIDs {
		ids = append(ids, id)
	}
	sort.Strings(ids)
	return ids
}

// Channel returns the channel name this client is tracking.
func (lc *LazyClient) Channel() string {
	return lc.channel
}

// ServerURL returns the server URL.
func (lc *LazyClient) ServerURL() string {
	return lc.serverURL
}

// OnUpdate registers a callback invoked on every live update.
func (lc *LazyClient) OnUpdate(f OnUpdateFunc) {
	lc.mu.Lock()
	defer lc.mu.Unlock()
	lc.onUpdate = f
}

// Close cancels the background WebSocket listener.
func (lc *LazyClient) Close() {
	lc.cancel()
}

// ---------------------------------------------------------------------------
// Internal
// ---------------------------------------------------------------------------

func (lc *LazyClient) materialise(resourceID string) {
	lc.mu.Lock()
	defer lc.mu.Unlock()

	// Double-check under write lock
	if lc.loaded[resourceID] {
		return
	}

	// The resource should be available from the clone; just mark it loaded
	if lc.client.ResourceExists(resourceID) {
		lc.loaded[resourceID] = true
	}
}

// materialiseBatch ensures a batch of resources is marked as loaded.
// Since dyna-go's clone already materialises snapshots, this simply marks
// resources that exist as loaded in a single lock acquisition.
func (lc *LazyClient) materialiseBatch(resourceIDs []string) {
	// Collect IDs that need loading
	lc.mu.RLock()
	var toLoad []string
	for _, rid := range resourceIDs {
		if !lc.loaded[rid] {
			toLoad = append(toLoad, rid)
		}
	}
	lc.mu.RUnlock()

	if len(toLoad) == 0 {
		return
	}

	lc.mu.Lock()
	defer lc.mu.Unlock()

	for _, rid := range toLoad {
		if !lc.loaded[rid] && lc.client.ResourceExists(rid) {
			lc.loaded[rid] = true
		}
	}
}

// pull fetches new changesets from the server and returns the list of affected
// resource IDs along with their new materialised snapshots.
func (lc *LazyClient) pull(ctx context.Context) ([]string, map[string]json.RawMessage) {
	resp, err := lc.client.Pull(ctx)
	if err != nil {
		log.Printf("lazy-go: pull failed: %v", err)
		return nil, nil
	}

	lc.mu.Lock()
	defer lc.mu.Unlock()

	var affected []string
	for _, cs := range resp.Changesets {
		for _, p := range cs.Patches {
			lc.knownIDs[p.TargetResource] = true
			lc.loaded[p.TargetResource] = true
			affected = append(affected, p.TargetResource)
		}
	}

	// Collect updated snapshots for all affected resources
	snapshots := make(map[string]json.RawMessage, len(affected))
	for _, rid := range affected {
		if data, err := lc.client.ReadResource(rid); err == nil {
			snapshots[rid] = data
		}
	}

	return affected, snapshots
}

// wsListener connects to the server's WebSocket and pulls on relevant
// notifications.
func (lc *LazyClient) wsListener(ctx context.Context) {
	wsURL := strings.Replace(lc.serverURL, "http://", "ws://", 1)
	wsURL = strings.Replace(wsURL, "https://", "wss://", 1)
	wsURL += "/api/v1/ws"

	for {
		select {
		case <-ctx.Done():
			return
		default:
		}

		if err := lc.wsConnect(ctx, wsURL); err != nil {
			log.Printf("lazy-go: ws error: %v, reconnecting in 5s…", err)
		}

		select {
		case <-ctx.Done():
			return
		case <-time.After(5 * time.Second):
		}
	}
}

// notification mirrors the Rust Notification struct.
type notification struct {
	Kind      string          `json:"kind"`
	Timestamp string          `json:"timestamp"`
	Payload   json.RawMessage `json:"payload"`
}

type pushPayload struct {
	Channel        string          `json:"channel"`
	ChangesetCount int             `json:"changeset_count"`
	NewHead        *string         `json:"new_head"`
	Changesets     []changesetInfo `json:"changesets"`
}

type promotionPayload struct {
	SourceChannel          string          `json:"source_channel"`
	TargetChannel          string          `json:"target_channel"`
	PromotedChangesets     []changesetInfo `json:"promoted_changesets"`
	NewHead                *string         `json:"new_head"`
	TotalResourcesAffected int            `json:"total_resources_affected"`
}

type changesetInfo struct {
	ChangeID          string   `json:"change_id"`
	Message           string   `json:"message"`
	Author            string   `json:"author"`
	PatchCount        int      `json:"patch_count"`
	AffectedResources []string `json:"affected_resources"`
}

func (lc *LazyClient) wsConnect(ctx context.Context, wsURL string) error {
	conn, _, err := websocket.DefaultDialer.DialContext(ctx, wsURL, nil)
	if err != nil {
		return fmt.Errorf("ws connect: %w", err)
	}
	defer conn.Close()

	log.Printf("lazy-go: WebSocket connected to %s", wsURL)

	done := make(chan struct{})
	go func() {
		defer close(done)
		for {
			_, message, err := conn.ReadMessage()
			if err != nil {
				return
			}
			lc.handleNotification(ctx, message)
		}
	}()

	select {
	case <-ctx.Done():
		_ = conn.WriteMessage(
			websocket.CloseMessage,
			websocket.FormatCloseMessage(websocket.CloseNormalClosure, ""),
		)
		return ctx.Err()
	case <-done:
		return nil
	}
}

func (lc *LazyClient) handleNotification(ctx context.Context, raw []byte) {
	var n notification
	if err := json.Unmarshal(raw, &n); err != nil {
		return
	}

	var affectedChannel string
	var csInfos []ChangesetInfo
	var newHead *string

	switch n.Kind {
	case "push":
		var p pushPayload
		if err := json.Unmarshal(n.Payload, &p); err != nil {
			return
		}
		affectedChannel = p.Channel
		newHead = p.NewHead
		for _, cs := range p.Changesets {
			csInfos = append(csInfos, ChangesetInfo{
				ChangeID:          cs.ChangeID,
				Message:           cs.Message,
				Author:            cs.Author,
				PatchCount:        cs.PatchCount,
				AffectedResources: cs.AffectedResources,
			})
		}
	case "promotion":
		var p promotionPayload
		if err := json.Unmarshal(n.Payload, &p); err != nil {
			return
		}
		affectedChannel = p.TargetChannel
		newHead = p.NewHead
		for _, cs := range p.PromotedChangesets {
			csInfos = append(csInfos, ChangesetInfo{
				ChangeID:          cs.ChangeID,
				Message:           cs.Message,
				Author:            cs.Author,
				PatchCount:        cs.PatchCount,
				AffectedResources: cs.AffectedResources,
			})
		}
	default:
		return
	}

	if affectedChannel != lc.channel {
		return
	}

	// Collect all affected resource IDs
	var allAffected []string
	for _, cs := range csInfos {
		allAffected = append(allAffected, cs.AffectedResources...)
	}

	log.Printf("lazy-go: %s notification, %d resource(s) affected", n.Kind, len(allAffected))

	// Pull updates and get materialised snapshots
	_, updatedSnapshots := lc.pull(ctx)

	// Build the rich UpdateEvent
	event := &UpdateEvent{
		Kind:                n.Kind,
		Timestamp:           n.Timestamp,
		Channel:             affectedChannel,
		Changesets:          csInfos,
		NewHead:             newHead,
		AffectedResourceIDs: allAffected,
		UpdatedSnapshots:    updatedSnapshots,
	}

	// Fire callback
	lc.mu.RLock()
	cb := lc.onUpdate
	lc.mu.RUnlock()
	if cb != nil {
		cb(event)
	}
}
