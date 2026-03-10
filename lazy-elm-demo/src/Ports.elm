port module Ports exposing (..)

{-| Ports for communicating with the lazy-wasm JavaScript layer.

Outgoing ports (Elm → JS) are commands.
Incoming ports (JS → Elm) are subscriptions.
-}

import Json.Decode as Decode
import Json.Encode as Encode


-- =========================================================================
-- Outgoing ports (Elm → JS commands)
-- =========================================================================


{-| Connect to a Dyna server on the given channel.
Expects a JSON object: { "serverUrl": "...", "channel": "..." }
-}
port connect : { serverUrl : String, channel : String } -> Cmd msg


{-| Request the list of all known resource IDs. -}
port listResources : () -> Cmd msg


{-| Fetch a single resource by ID. -}
port getResource : String -> Cmd msg


{-| Fetch all resources. -}
port getAllResources : () -> Cmd msg


{-| Stream all resources via for_each_all. -}
port streamAllResources : () -> Cmd msg



-- =========================================================================
-- Incoming ports (JS → Elm subscriptions)
-- =========================================================================


{-| Connection result: { success: bool, error: string|null, channel: string } -}
port onConnectResult : (Decode.Value -> msg) -> Sub msg


{-| Resource list result: string[] -}
port onResourceList : (Decode.Value -> msg) -> Sub msg


{-| Single resource result: { id: string, value: object } -}
port onResourceResult : (Decode.Value -> msg) -> Sub msg


{-| All resources result: { id: value, ... } -}
port onAllResources : (Decode.Value -> msg) -> Sub msg


{-| Streamed resource (called per resource): { id: string, value: object } -}
port onStreamedResource : (Decode.Value -> msg) -> Sub msg


{-| Stream complete signal -}
port onStreamComplete : (Decode.Value -> msg) -> Sub msg


{-| Live update event (UpdateEvent from lazy-wasm) -}
port onLiveUpdate : (Decode.Value -> msg) -> Sub msg
