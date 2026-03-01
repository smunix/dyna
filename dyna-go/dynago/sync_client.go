package dynago

import (
	"bytes"
	"compress/gzip"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"strings"
	"time"

	"github.com/gorilla/websocket"
)

// SyncClient communicates with a dyna-server over HTTP.
type SyncClient struct {
	baseURL    string
	httpClient *http.Client
}

// NewSyncClient creates a SyncClient for the given server URL.
func NewSyncClient(baseURL string) *SyncClient {
	return &SyncClient{
		baseURL: strings.TrimRight(baseURL, "/"),
		httpClient: &http.Client{
			Timeout: 30 * time.Second,
		},
	}
}

// ---------------------------------------------------------------------------
// Push
// ---------------------------------------------------------------------------

// Push sends changesets to the remote server.
func (c *SyncClient) Push(ctx context.Context, req *PushRequest) (*PushResponse, error) {
	var resp PushResponse
	if err := c.postJSON(ctx, "/api/v1/push", req, &resp); err != nil {
		return nil, err
	}
	if !resp.Success {
		msg := "unknown error"
		if resp.Error != nil {
			msg = *resp.Error
		}
		return nil, fmt.Errorf("push rejected: %s", msg)
	}
	return &resp, nil
}

// ---------------------------------------------------------------------------
// Pull
// ---------------------------------------------------------------------------

// Pull fetches new changesets from the remote server.
func (c *SyncClient) Pull(ctx context.Context, req *PullRequest) (*PullResponse, error) {
	var resp PullResponse
	if err := c.postJSON(ctx, "/api/v1/pull", req, &resp); err != nil {
		return nil, err
	}
	return &resp, nil
}

// ---------------------------------------------------------------------------
// Clone
// ---------------------------------------------------------------------------

// CloneRepo clones the entire repository from the remote server.
func (c *SyncClient) CloneRepo(ctx context.Context, req *CloneRequest) (*CloneResponse, error) {
	var resp CloneResponse
	if err := c.postJSON(ctx, "/api/v1/clone", req, &resp); err != nil {
		return nil, err
	}
	return &resp, nil
}

// ---------------------------------------------------------------------------
// Channels
// ---------------------------------------------------------------------------

// ListRemoteChannels fetches all channels from the remote server.
func (c *SyncClient) ListRemoteChannels(ctx context.Context) (*ListChannelsResponse, error) {
	var resp ListChannelsResponse
	if err := c.getJSON(ctx, "/api/v1/channels", &resp); err != nil {
		return nil, err
	}
	return &resp, nil
}

// CreateRemoteChannel creates a channel on the remote server.
func (c *SyncClient) CreateRemoteChannel(ctx context.Context, req *CreateChannelRequest) (*CreateChannelResponse, error) {
	var resp CreateChannelResponse
	if err := c.postJSON(ctx, "/api/v1/channels", req, &resp); err != nil {
		return nil, err
	}
	return &resp, nil
}

// ---------------------------------------------------------------------------
// Promote
// ---------------------------------------------------------------------------

// Promote promotes changesets from one channel to another on the remote.
func (c *SyncClient) Promote(ctx context.Context, req *PromoteRequest) (*PromoteResponse, error) {
	var resp PromoteResponse
	if err := c.postJSON(ctx, "/api/v1/promote", req, &resp); err != nil {
		return nil, err
	}
	if !resp.Success {
		msg := "unknown error"
		if resp.Error != nil {
			msg = *resp.Error
		}
		return nil, fmt.Errorf("promote rejected: %s", msg)
	}
	return &resp, nil
}

// ---------------------------------------------------------------------------
// Resource history
// ---------------------------------------------------------------------------

// ResourceHistory queries the change history of a specific resource.
func (c *SyncClient) ResourceHistory(ctx context.Context, resourceID string) (*ResourceHistoryResponse, error) {
	var resp ResourceHistoryResponse
	endpoint := fmt.Sprintf("/api/v1/resources/%s/history", resourceID)
	if err := c.getJSON(ctx, endpoint, &resp); err != nil {
		return nil, err
	}
	return &resp, nil
}

// ---------------------------------------------------------------------------
// Health
// ---------------------------------------------------------------------------

// Health checks the remote server health.
func (c *SyncClient) Health(ctx context.Context) (*HealthResponse, error) {
	var resp HealthResponse
	if err := c.getJSON(ctx, "/api/v1/health", &resp); err != nil {
		return nil, err
	}
	return &resp, nil
}

// ---------------------------------------------------------------------------
// WebSocket notifications
// ---------------------------------------------------------------------------

// NotificationHandler is called for each notification received.
type NotificationHandler func(data json.RawMessage)

// SubscribeNotifications connects to the server's WebSocket endpoint and
// calls handler for each notification. It blocks until the context is
// cancelled or the connection drops.
func (c *SyncClient) SubscribeNotifications(ctx context.Context, handler NotificationHandler) error {
	wsURL := strings.Replace(c.baseURL, "http://", "ws://", 1)
	wsURL = strings.Replace(wsURL, "https://", "wss://", 1)
	wsURL += "/api/v1/ws"

	conn, _, err := websocket.DefaultDialer.DialContext(ctx, wsURL, nil)
	if err != nil {
		return fmt.Errorf("websocket connect failed: %w", err)
	}
	defer conn.Close()

	done := make(chan struct{})
	go func() {
		defer close(done)
		for {
			_, message, err := conn.ReadMessage()
			if err != nil {
				return
			}
			handler(json.RawMessage(message))
		}
	}()

	select {
	case <-ctx.Done():
		conn.WriteMessage(websocket.CloseMessage,
			websocket.FormatCloseMessage(websocket.CloseNormalClosure, ""))
		return ctx.Err()
	case <-done:
		return nil
	}
}

// ---------------------------------------------------------------------------
// HTTP helpers (with gzip compression)
// ---------------------------------------------------------------------------

func (c *SyncClient) postJSON(ctx context.Context, endpoint string, reqBody, respBody interface{}) error {
	data, err := json.Marshal(reqBody)
	if err != nil {
		return fmt.Errorf("marshal request: %w", err)
	}

	// Gzip compress
	var buf bytes.Buffer
	gz := gzip.NewWriter(&buf)
	if _, err := gz.Write(data); err != nil {
		return err
	}
	gz.Close()

	url := c.baseURL + endpoint
	req, err := http.NewRequestWithContext(ctx, "POST", url, &buf)
	if err != nil {
		return err
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Content-Encoding", "gzip")
	req.Header.Set("Accept-Encoding", "gzip")

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return fmt.Errorf("request to %s failed: %w", endpoint, err)
	}
	defer resp.Body.Close()

	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		body, _ := io.ReadAll(resp.Body)
		return fmt.Errorf("%s failed (HTTP %d): %s", endpoint, resp.StatusCode, string(body))
	}

	body, err := readTransparent(resp)
	if err != nil {
		return fmt.Errorf("read response: %w", err)
	}

	return json.Unmarshal(body, respBody)
}

func (c *SyncClient) getJSON(ctx context.Context, endpoint string, respBody interface{}) error {
	url := c.baseURL + endpoint
	req, err := http.NewRequestWithContext(ctx, "GET", url, nil)
	if err != nil {
		return err
	}
	req.Header.Set("Accept-Encoding", "gzip")

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return fmt.Errorf("request to %s failed: %w", endpoint, err)
	}
	defer resp.Body.Close()

	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		body, _ := io.ReadAll(resp.Body)
		return fmt.Errorf("%s failed (HTTP %d): %s", endpoint, resp.StatusCode, string(body))
	}

	body, err := readTransparent(resp)
	if err != nil {
		return fmt.Errorf("read response: %w", err)
	}

	return json.Unmarshal(body, respBody)
}

// readTransparent reads the response body, transparently decompressing gzip.
func readTransparent(resp *http.Response) ([]byte, error) {
	var reader io.Reader = resp.Body
	if resp.Header.Get("Content-Encoding") == "gzip" {
		gz, err := gzip.NewReader(resp.Body)
		if err != nil {
			return nil, err
		}
		defer gz.Close()
		reader = gz
	}
	return io.ReadAll(reader)
}
