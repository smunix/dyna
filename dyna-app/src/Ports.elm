port module Ports exposing (..)

{-| Ports for communicating with the dyna-wasm JavaScript layer.

All outgoing ports (Elm → JS) are commands.
All incoming ports (JS → Elm) are subscriptions.
-}

import Json.Decode as Decode
import Json.Encode as Encode


-- =========================================================================
-- Outgoing ports (Elm → JS commands)
-- =========================================================================


port initRepo : String -> Cmd msg


port cloneRepo : String -> Cmd msg


port writeFile : { path : String, content : String } -> Cmd msg


port readFile : String -> Cmd msg


port deleteFile : String -> Cmd msg


port addFile : String -> Cmd msg


port addDelete : String -> Cmd msg


port requestStatus : () -> Cmd msg


port commitChanges : String -> Cmd msg


port pushChanges : () -> Cmd msg


port promoteChanges : String -> Cmd msg


port requestChannels : () -> Cmd msg


port createChannel : String -> Cmd msg


port switchChannel : String -> Cmd msg


port requestLog : Int -> Cmd msg


port requestHistory : String -> Cmd msg


port listFiles : () -> Cmd msg


port listSnapshots : () -> Cmd msg


port connectNotifications : () -> Cmd msg


port disconnectNotifications : () -> Cmd msg



-- =========================================================================
-- Incoming ports (JS → Elm subscriptions)
-- =========================================================================


port onInitResult : (Decode.Value -> msg) -> Sub msg


port onCloneResult : (Decode.Value -> msg) -> Sub msg


port onWriteResult : (Decode.Value -> msg) -> Sub msg


port onReadResult : (Decode.Value -> msg) -> Sub msg


port onDeleteResult : (Decode.Value -> msg) -> Sub msg


port onAddResult : (Decode.Value -> msg) -> Sub msg


port onStatusResult : (Decode.Value -> msg) -> Sub msg


port onCommitResult : (Decode.Value -> msg) -> Sub msg


port onPushResult : (Decode.Value -> msg) -> Sub msg


port onPromoteResult : (Decode.Value -> msg) -> Sub msg


port onChannelsResult : (Decode.Value -> msg) -> Sub msg


port onCreateChannelResult : (Decode.Value -> msg) -> Sub msg


port onSwitchChannelResult : (Decode.Value -> msg) -> Sub msg


port onLogResult : (Decode.Value -> msg) -> Sub msg


port onHistoryResult : (Decode.Value -> msg) -> Sub msg


port onListFilesResult : (Decode.Value -> msg) -> Sub msg


port onListSnapshotsResult : (Decode.Value -> msg) -> Sub msg


port onNotification : (String -> msg) -> Sub msg
