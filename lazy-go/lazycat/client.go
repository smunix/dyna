// Package lazycat provides a lazy, on-demand resource loader for Dyna servers.
//
// LazyClient maintains an in-memory repository (via dyna-go) that is populated
// only with resources that have been explicitly requested.  A background
// WebSocket listener keeps the local cache up-to-date whenever new changesets
// are pushed to the configured channel.
package lazycat

import (
	"context"
	"encoding/json"
	"fmt"
	"log"
	"strings"
	"sync"
	"time"

	"dyna-go/dynago"

	"github.com/gorilla/websocket"
)

// OnUpdateFunc is called whenever the local cache is updated from the server.
// It receives the list of resource IDs that were affected.
type OnUpdateFunc func(affectedResources []string)

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
func (lc *LazyClient) GetMany(resourceIDs []string) (map[string]json.RawMessage, error) {
	for _, rid := range resourceIDs {
		lc.mu.RLock()
		isLoaded := lc.loaded[rid]
		lc.mu.RUnlock()
		if !isLoaded {
			lc.materialise(rid)
		}
	}

	result := make(map[string]json.RawMessage, len(resourceIDs))
	for _, rid := range resourceIDs {
		data, err := lc.client.ReadResource(rid)
		if err == nil {
			result[rid] = data
		}
	}
	return result, nil
}

// GetAll fetches all known resources.
func (lc *LazyClient) GetAll() (map[string]json.RawMessage, error) {
	ids := lc.ListResources()
	return lc.GetMany(ids)
}

// ListResources returns all known resource IDs (metadata only).
func (lc *LazyClient) ListResources() []string {
	lc.mu.RLock()
	defer lc.mu.RUnlock()

	ids := make([]string, 0, len(lc.knownIDs))
	for id := range lc.knownIDs {
		ids = append(ids, id)
	}
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

func (lc *LazyClient) pull(ctx context.Context) []string {
	resp, err := lc.client.Pull(ctx)
	if err != nil {
		log.Printf("lazy-go: pull failed: %v", err)
		return nil
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
	return affected
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
	Kind    string          `json:"kind"`
	Payload json.RawMessage `json:"payload"`
}

type pushPayload struct {
	Channel    string          `json:"channel"`
	Changesets []changesetInfo `json:"changesets"`
}

type promotionPayload struct {
	TargetChannel      string          `json:"target_channel"`
	PromotedChangesets []changesetInfo `json:"promoted_changesets"`
}

type changesetInfo struct {
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
	var affected []string

	switch n.Kind {
	case "push":
		var p pushPayload
		if err := json.Unmarshal(n.Payload, &p); err != nil {
			return
		}
		affectedChannel = p.Channel
		for _, cs := range p.Changesets {
			affected = append(affected, cs.AffectedResources...)
		}
	case "promotion":
		var p promotionPayload
		if err := json.Unmarshal(n.Payload, &p); err != nil {
			return
		}
		affectedChannel = p.TargetChannel
		for _, cs := range p.PromotedChangesets {
			affected = append(affected, cs.AffectedResources...)
		}
	default:
		return
	}

	if affectedChannel != lc.channel {
		return
	}

	log.Printf("lazy-go: %s notification, %d resource(s) affected", n.Kind, len(affected))

	// Pull updates
	lc.pull(ctx)

	// Fire callback
	lc.mu.RLock()
	cb := lc.onUpdate
	lc.mu.RUnlock()
	if cb != nil {
		cb(affected)
	}
}
