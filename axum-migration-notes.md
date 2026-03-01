# Axum 0.7 → 0.8 Migration Notes

## Confirmed from axum 0.8.8 Cargo.toml

- tower-http = "0.6.0" (we have 0.5, need to bump to 0.6)
- tokio-tungstenite = "0.28.0" (ws feature)
- axum-core = "0.5.5"

## Breaking Changes to Fix

1. Path param syntax: `/:param` → `/{param}`
   - api.rs line 338: `/api/v1/changesets/:change_id` → `/api/v1/changesets/{change_id}`
   - api.rs line 341: `/api/v1/history/:resource_id` → `/api/v1/history/{resource_id}`

2. tower-http: 0.5 → 0.6

3. WebSocket Message::Text: In axum 0.8 (tokio-tungstenite 0.28), Message::Text uses Utf8Bytes instead of String
   - ws.rs line 90: `Message::Text(msg.into())` — may need adjustment

4. axum::serve may have changed (need to check if `serve(listener, app)` still works)
