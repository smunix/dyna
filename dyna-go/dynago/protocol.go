package dynago

import "encoding/json"

// ---------------------------------------------------------------------------
// Push
// ---------------------------------------------------------------------------

// PushRequest is sent to POST /api/v1/push.
type PushRequest struct {
	Channel    string      `json:"channel"`
	Changesets []Changeset `json:"changesets"`
}

// PushResponse is returned from POST /api/v1/push.
type PushResponse struct {
	Success bool    `json:"success"`
	NewHead *string `json:"new_head"`
	Error   *string `json:"error"`
}

// ---------------------------------------------------------------------------
// Pull
// ---------------------------------------------------------------------------

// PullRequest is sent to POST /api/v1/pull.
type PullRequest struct {
	Channel       string  `json:"channel"`
	SinceChangeID *string `json:"since_change_id"`
}

// PullResponse is returned from POST /api/v1/pull.
type PullResponse struct {
	Channel     Channel     `json:"channel"`
	Changesets  []Changeset `json:"changesets"`
	CurrentHead *string     `json:"current_head"`
}

// ---------------------------------------------------------------------------
// Clone
// ---------------------------------------------------------------------------

// CloneRequest is sent to POST /api/v1/clone.
type CloneRequest struct {
	Channel *string `json:"channel,omitempty"`
}

// CloneResponse is returned from POST /api/v1/clone.
type CloneResponse struct {
	Channels   []Channel                  `json:"channels"`
	Changesets []Changeset                `json:"changesets"`
	Snapshots  map[string]json.RawMessage `json:"snapshots"`
}

// ---------------------------------------------------------------------------
// Promote
// ---------------------------------------------------------------------------

// PromoteRequest is sent to POST /api/v1/promote.
type PromoteRequest struct {
	SourceChannel string `json:"source_channel"`
	TargetChannel string `json:"target_channel"`
}

// PromoteResponse is returned from POST /api/v1/promote.
type PromoteResponse struct {
	Success             bool        `json:"success"`
	PromotedChangesets  []Changeset `json:"promoted_changesets"`
	NewHead             *string     `json:"new_head"`
	Error               *string     `json:"error"`
}

// ---------------------------------------------------------------------------
// Channels
// ---------------------------------------------------------------------------

// ListChannelsResponse is returned from GET /api/v1/channels.
type ListChannelsResponse struct {
	Channels []Channel `json:"channels"`
}

// CreateChannelRequest is sent to POST /api/v1/channels.
type CreateChannelRequest struct {
	Name     string  `json:"name"`
	ForkFrom *string `json:"fork_from,omitempty"`
}

// CreateChannelResponse is returned from POST /api/v1/channels.
type CreateChannelResponse struct {
	Channel *Channel `json:"channel"`
	Error   *string  `json:"error"`
}

// ---------------------------------------------------------------------------
// Changeset query
// ---------------------------------------------------------------------------

// GetChangesetRequest is sent to query a single changeset.
type GetChangesetRequest struct {
	ChangeID string `json:"change_id"`
}

// GetChangesetResponse is returned from a changeset query.
type GetChangesetResponse struct {
	Changeset *Changeset `json:"changeset"`
	Error     *string    `json:"error"`
}

// ---------------------------------------------------------------------------
// Resource history
// ---------------------------------------------------------------------------

// ResourceHistoryResponse is returned from GET /api/v1/resources/{id}/history.
type ResourceHistoryResponse struct {
	ResourceID string                 `json:"resource_id"`
	Entries    []ResourceHistoryEntry `json:"entries"`
	Error      *string                `json:"error"`
}

// ResourceHistoryEntry is a single entry in a resource's change history.
type ResourceHistoryEntry struct {
	ChangeID   string            `json:"change_id"`
	CommitHash string            `json:"commit_hash"`
	Message    string            `json:"message"`
	Author     string            `json:"author"`
	Timestamp  string            `json:"timestamp"`
	Channel    string            `json:"channel"`
	Operations []json.RawMessage `json:"operations"`
}

// ---------------------------------------------------------------------------
// Health
// ---------------------------------------------------------------------------

// HealthResponse is returned from GET /api/v1/health.
type HealthResponse struct {
	Status        string `json:"status"`
	Version       string `json:"version"`
	UptimeSeconds uint64 `json:"uptime_seconds"`
}

// ErrorResponse is a generic error body.
type ErrorResponse struct {
	Error string `json:"error"`
	Code  string `json:"code"`
}
