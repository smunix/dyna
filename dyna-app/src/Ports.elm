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


port stageAll : () -> Cmd msg


port syncMain : () -> Cmd msg


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


port setUser : { name : String, email : String } -> Cmd msg


port connectNotifications : () -> Cmd msg


port disconnectNotifications : () -> Cmd msg


-- Feature ports

port getSnapshot : String -> Cmd msg


port cherryPick : String -> Cmd msg


port restoreFromChannel : { resourceId : String, channel : String } -> Cmd msg


port restoreFromChangeset : { resourceId : String, changeId : String } -> Cmd msg


port getChangeset : String -> Cmd msg


port logForChannel : String -> Cmd msg


port listChannelResources : String -> Cmd msg


port getSnapshotFromChannel : { resourceId : String, channel : String } -> Cmd msg


port listRemoteChannels : () -> Cmd msg


port pullChannel : String -> Cmd msg


port unstageResource : String -> Cmd msg


port unstageAll : () -> Cmd msg


port deleteChannel : { name : String, force : Bool } -> Cmd msg


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


port onSnapshotResult : (Decode.Value -> msg) -> Sub msg


port onCherryPickResult : (Decode.Value -> msg) -> Sub msg


port onRestoreResult : (Decode.Value -> msg) -> Sub msg


port onChangesetResult : (Decode.Value -> msg) -> Sub msg


port onChannelLogResult : (Decode.Value -> msg) -> Sub msg


port onChannelResourcesResult : (Decode.Value -> msg) -> Sub msg


port onSnapshotFromChannelResult : (Decode.Value -> msg) -> Sub msg


port onRemoteChannelsResult : (Decode.Value -> msg) -> Sub msg


port onPullChannelResult : (Decode.Value -> msg) -> Sub msg


port onStageAllResult : (Decode.Value -> msg) -> Sub msg


port onSyncMainResult : (Decode.Value -> msg) -> Sub msg


port onUnstageResult : (Decode.Value -> msg) -> Sub msg


port onDeleteChannelResult : (Decode.Value -> msg) -> Sub msg
