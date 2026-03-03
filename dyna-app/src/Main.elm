module Main exposing (main)

{-| Dyna — Collaborative JSON Editor

A browser-based UI for the Dyna distributed CRUD system.
Uses dyna-wasm (via ports) for all repository operations.
-}

import Browser
import Html exposing (..)
import Html.Attributes exposing (..)
import Html.Events exposing (..)
import Json.Decode as Decode
import Json.Encode as Encode
import Ports



-- =========================================================================
-- MODEL
-- =========================================================================


type Page
    = SetupPage
    | ResourcesPage
    | EditorPage
    | HistoryPage
    | LogPage
    | ImportPage
    | ChangesetDetailPage


type alias StagedFile =
    { resourceId : String
    , ops : Int
    , kind : String
    }


type alias StatusInfo =
    { channel : String
    , staged : List StagedFile
    , modified : List String
    , deleted : List String
    , unstagedOnStaged : List String
    , conflicts : List String
    }


type alias ChannelInfo =
    { name : String
    , changesetCount : Int
    , head : Maybe String
    , isCurrent : Bool
    }


type alias LogEntry =
    { changeId : String
    , commitHash : String
    , message : String
    , author : String
    , createdAt : String
    , patchCount : Int
    , immutable : Bool
    }


type alias HistoryEntry =
    { changeId : String
    , commitHash : String
    , message : String
    , author : String
    , timestamp : String
    , channel : String
    }


type alias ChangesetSummary =
    { changeId : String
    , message : String
    , author : String
    , patchCount : Int
    , affectedResources : List String
    }


type alias Notification =
    { kind : String
    , channel : String
    , title : String
    , body : String
    , changesets : List ChangesetSummary
    , id : Int
    , dismissed : Bool
    , hidden : Bool
    }


type alias ChangesetDetail =
    { changeId : String
    , commitHash : String
    , message : String
    , author : String
    , createdAt : String
    , patches : List PatchInfo
    }


type alias PatchInfo =
    { targetResource : String
    , opsCount : Int
    , hasSnapshot : Bool
    , operations : List PatchOp
    }


type alias PatchOp =
    { op : String
    , path : String
    , value : Maybe String
    , from : Maybe String
    }


type ResourceSort
    = SortByIdAsc
    | SortByIdDesc


type ChannelSort
    = ChannelSortAlpha
    | ChannelSortByChangesets


type alias Model =
    { page : Page
    , serverUrl : String
    , connected : Bool
    , status : StatusInfo
    , channels : List ChannelInfo
    , remoteChannels : List String
    , resources : List String
    , snapshots : List String

    -- Editor state
    , editResourceId : String
    , editContent : String
    , editJsonError : Maybe String
    , editIsNew : Bool

    -- History
    , historyResourceId : String
    , historyEntries : List HistoryEntry

    -- Log
    , logEntries : List LogEntry

    -- User
    , userId : String

    -- Tooltip
    , tooltipResourceId : String
    , tooltipContent : String

    -- Channel tooltip
    , channelTooltipName : String

    -- Import
    , importSourceChannel : String
    , importChannelResources : List String
    , importChannelLog : List LogEntry

    -- Changeset detail
    , changesetDetail : Maybe ChangesetDetail

    -- Restore dialog
    , showRestoreDialog : Bool
    , restoreResourceId : String
    , restoreMode : String
    , restoreChannel : String
    , restoreChangeId : String

    -- Dialogs
    , showCommitDialog : Bool
    , commitMessage : String
    , showNewChannelDialog : Bool
    , newChannelName : String
    , showPromoteDialog : Bool
    , promoteChannel : String

    -- Notifications
    , notifications : List Notification
    , nextNotifId : Int
    , notificationsVisible : Bool

    -- Feedback
    , flashMessage : Maybe String
    , flashIsError : Bool

    -- Expanded patch operations
    , expandedPatches : List String

    -- Collapsible sections
    , stagedSectionOpen : Bool
    , workingSectionOpen : Bool
    , committedSectionOpen : Bool

    -- Resource filtering/sorting
    , resourceFilter : String
    , resourceSort : ResourceSort

    -- Channel filtering/sorting
    , channelFilter : String
    , channelSort : ChannelSort
    , showAllChannels : Bool
    }


emptyStatus : StatusInfo
emptyStatus =
    { channel = "main"
    , staged = []
    , modified = []
    , deleted = []
    , unstagedOnStaged = []
    , conflicts = []
    }


init : Decode.Value -> ( Model, Cmd msg )
init flags =
    let
        serverUrl =
            flags
                |> Decode.decodeValue (Decode.field "serverUrl" Decode.string)
                |> Result.withDefault "http://localhost:8080"
    in
    ( { page = SetupPage
      , serverUrl = serverUrl
      , connected = False
      , status = emptyStatus
      , channels = []
      , remoteChannels = []
      , resources = []
      , snapshots = []
      , editResourceId = ""
      , editContent = ""
      , editJsonError = Nothing
      , editIsNew = False
      , historyResourceId = ""
      , historyEntries = []
      , logEntries = []
      , userId = ""
      , tooltipResourceId = ""
      , tooltipContent = ""
      , channelTooltipName = ""
      , importSourceChannel = ""
      , importChannelResources = []
      , importChannelLog = []
      , changesetDetail = Nothing
      , showRestoreDialog = False
      , restoreResourceId = ""
      , restoreMode = "channel"
      , restoreChannel = ""
      , restoreChangeId = ""
      , showCommitDialog = False
      , commitMessage = ""
      , showNewChannelDialog = False
      , newChannelName = ""
      , showPromoteDialog = False
      , promoteChannel = ""
      , notifications = []
      , nextNotifId = 0
      , notificationsVisible = True
      , flashMessage = Nothing
      , flashIsError = False
      , expandedPatches = []
      , stagedSectionOpen = True
      , workingSectionOpen = True
      , committedSectionOpen = False
      , resourceFilter = ""
      , resourceSort = SortByIdAsc
      , channelFilter = ""
      , channelSort = ChannelSortAlpha
      , showAllChannels = False
      }
    , Cmd.none
    )



-- =========================================================================
-- UPDATE
-- =========================================================================


type Msg
    = -- Navigation
      GoToPage Page
      -- Setup
    | UpdateServerUrl String
    | ConnectToServer
    | CloneFromServer
    | GotInitResult Decode.Value
    | GotCloneResult Decode.Value
      -- Resources
    | RefreshAll
    | GotListFiles Decode.Value
    | GotListSnapshots Decode.Value
    | GotStatus Decode.Value
    | GotChannels Decode.Value
      -- Editor
    | OpenResource String
    | OpenNewResource
    | UpdateResourceId String
    | UpdateContent String
    | SaveResource
    | DeleteResource
    | StageResource
    | StageDelete
    | GotReadResult Decode.Value
    | GotWriteResult Decode.Value
    | GotDeleteResult Decode.Value
    | GotAddResult Decode.Value
      -- Commit / Push / Promote
    | ShowCommitDialog
    | HideCommitDialog
    | UpdateCommitMessage String
    | DoCommit
    | GotCommitResult Decode.Value
    | DoPush
    | GotPushResult Decode.Value
    | ShowPromoteDialog
    | HidePromoteDialog
    | UpdatePromoteChannel String
    | DoPromote
    | GotPromoteResult Decode.Value
      -- Channels
    | ShowNewChannelDialog
    | HideNewChannelDialog
    | UpdateNewChannelName String
    | DoCreateChannel
    | GotCreateChannelResult Decode.Value
    | DoSwitchChannel String
    | GotSwitchChannelResult Decode.Value
      -- History
    | OpenHistory String
    | GotHistoryResult Decode.Value
      -- Log
    | RefreshLog
    | GotLogResult Decode.Value
      -- Tooltip
    | RequestTooltip String
    | ClearTooltip
    | GotSnapshotResult Decode.Value
      -- Channel tooltip
    | ShowChannelTooltip String
    | HideChannelTooltip
      -- Import
    | GoToImport
    | UpdateImportChannel String
    | LoadImportChannel
    | GotChannelResources Decode.Value
    | GotChannelLog Decode.Value
    | ImportResourceFromChannel String
    | ImportChangeset String
    | GotCherryPickResult Decode.Value
      -- Changeset detail
    | ViewChangeset String
    | GotChangesetResult Decode.Value
    | ImportResourceFromChangeset String String
      -- Restore
    | ShowRestoreDialog String
    | HideRestoreDialog
    | UpdateRestoreMode String
    | UpdateRestoreChannel String
    | UpdateRestoreChangeId String
    | DoRestore
    | GotRestoreResult Decode.Value
      -- Notifications
    | GotNotification String
    | DismissNotification Int
    | HideNotification Int
    | ToggleNotificationsVisible
    | DismissAllNotifications
    | PullNotifChannel String
    | ImportNotifChangeset String String
      -- Remote channels
    | SearchRemoteChannels
    | GotRemoteChannels Decode.Value
    | PullRemoteChannel String
    | GotPullChannelResult Decode.Value
      -- User
    | UpdateUserId String
      -- Stage All / Commit All
    | StageAllWorking
    | GotStageAllResult Decode.Value
      -- Unstage
    | UnstageResource String
    | UnstageAllStaged
    | GotUnstageResult Decode.Value
      -- Sync main channel
    | SyncMain
    | GotSyncMainResult Decode.Value
      -- Patch operations toggle
    | TogglePatchOps String
      -- Collapsible sections
    | ToggleStagedSection
    | ToggleWorkingSection
    | ToggleCommittedSection
      -- Resource filter/sort
    | UpdateResourceFilter String
    | SetResourceSort ResourceSort
      -- Channel filter/sort
    | UpdateChannelFilter String
    | SetChannelSort ChannelSort
    | ToggleShowAllChannels
      -- Flash
    | DismissFlash
      -- Misc
    | NoOp


update : Msg -> Model -> ( Model, Cmd Msg )
update msg model =
    case msg of
        -- Navigation
        GoToPage page ->
            let
                cmd =
                    case page of
                        LogPage ->
                            Ports.requestLog 50

                        ResourcesPage ->
                            Cmd.batch
                                [ Ports.listSnapshots ()
                                , Ports.listFiles ()
                                , Ports.requestStatus ()
                                , Ports.requestChannels ()
                                ]

                        _ ->
                            Cmd.none
            in
            ( { model | page = page }, cmd )

        -- Setup
        UpdateServerUrl url ->
            ( { model | serverUrl = url }, Cmd.none )

        ConnectToServer ->
            ( model, Ports.initRepo model.serverUrl )

        CloneFromServer ->
            ( model, Ports.cloneRepo model.serverUrl )

        GotInitResult val ->
            case Decode.decodeValue (Decode.field "success" Decode.bool) val of
                Ok True ->
                    ( { model | connected = True, page = ResourcesPage }
                    , Cmd.batch
                        [ Ports.requestStatus ()
                        , Ports.requestChannels ()
                        , Ports.listSnapshots ()
                        , Ports.listFiles ()
                        , Ports.connectNotifications ()
                        , Ports.syncMain ()
                        ]
                    )

                _ ->
                    let
                        detail =
                            val
                                |> Decode.decodeValue (Decode.field "error" Decode.string)
                                |> Result.withDefault "unknown error"
                    in
                    ( { model | flashMessage = Just ("Failed to initialise repository: " ++ detail), flashIsError = True }, Cmd.none )

        GotCloneResult val ->
            case Decode.decodeValue (Decode.field "success" Decode.bool) val of
                Ok True ->
                    ( { model | connected = True, page = ResourcesPage }
                    , Cmd.batch
                        [ Ports.requestStatus ()
                        , Ports.requestChannels ()
                        , Ports.listSnapshots ()
                        , Ports.listFiles ()
                        , Ports.connectNotifications ()
                        , Ports.syncMain ()
                        ]
                    )

                _ ->
                    let
                        detail =
                            val
                                |> Decode.decodeValue (Decode.field "error" Decode.string)
                                |> Result.withDefault "unknown error"
                    in
                    ( { model | flashMessage = Just ("Failed to clone repository: " ++ detail), flashIsError = True }, Cmd.none )

        -- Resources
        RefreshAll ->
            ( model
            , Cmd.batch
                [ Ports.listSnapshots ()
                , Ports.listFiles ()
                , Ports.requestStatus ()
                , Ports.requestChannels ()
                ]
            )

        GotListFiles val ->
            case Decode.decodeValue (Decode.list Decode.string) val of
                Ok files ->
                    ( { model | resources = List.map filePathToResourceId files }, Cmd.none )

                Err _ ->
                    ( model, Cmd.none )

        GotListSnapshots val ->
            case Decode.decodeValue (Decode.list Decode.string) val of
                Ok ids ->
                    ( { model | snapshots = ids }, Cmd.none )

                Err _ ->
                    ( model, Cmd.none )

        GotStatus val ->
            case decodeStatus val of
                Ok status ->
                    ( { model | status = status }, Cmd.none )

                Err _ ->
                    ( model, Cmd.none )

        GotChannels val ->
            case Decode.decodeValue (Decode.list decodeChannelInfo) val of
                Ok channels ->
                    ( { model | channels = channels }, Cmd.none )

                Err _ ->
                    ( model, Cmd.none )

        -- Editor
        OpenResource resourceId ->
            ( { model
                | editResourceId = resourceId
                , editContent = ""
                , editJsonError = Nothing
                , editIsNew = False
                , page = EditorPage
              }
            , Ports.readFile resourceId
            )

        OpenNewResource ->
            ( { model
                | editResourceId = ""
                , editContent = "{\n  \n}"
                , editJsonError = Nothing
                , editIsNew = True
                , page = EditorPage
              }
            , Cmd.none
            )

        UpdateResourceId rid ->
            ( { model | editResourceId = rid }, Cmd.none )

        UpdateContent content ->
            let
                jsonError =
                    case Decode.decodeString Decode.value content of
                        Ok _ ->
                            Nothing

                        Err err ->
                            Just (Decode.errorToString err)
            in
            ( { model | editContent = content, editJsonError = jsonError }, Cmd.none )

        SaveResource ->
            if String.isEmpty model.editResourceId then
                ( { model | flashMessage = Just "Resource ID is required", flashIsError = True }, Cmd.none )

            else
                case model.editJsonError of
                    Just _ ->
                        ( { model | flashMessage = Just "Fix JSON errors before saving", flashIsError = True }, Cmd.none )

                    Nothing ->
                        let
                            path =
                                String.replace "." "/" model.editResourceId ++ ".json"
                        in
                        ( model, Ports.writeFile { path = path, content = model.editContent } )

        DeleteResource ->
            ( model, Ports.deleteFile model.editResourceId )

        StageResource ->
            ( model, Ports.addFile model.editResourceId )

        StageDelete ->
            ( model, Ports.addDelete model.editResourceId )

        GotReadResult val ->
            let
                content =
                    val
                        |> Decode.decodeValue (Decode.field "content" Decode.string)
                        |> Result.withDefault ""

                error =
                    val
                        |> Decode.decodeValue (Decode.field "error" (Decode.nullable Decode.string))
                        |> Result.withDefault Nothing
            in
            case error of
                Just _ ->
                    ( { model | editContent = "{\n  \n}", editIsNew = True }, Cmd.none )

                Nothing ->
                    ( { model | editContent = content, editIsNew = False }, Cmd.none )

        GotWriteResult val ->
            case Decode.decodeValue (Decode.field "success" Decode.bool) val of
                Ok True ->
                    ( { model | flashMessage = Just "File saved", flashIsError = False }
                    , Ports.requestStatus ()
                    )

                _ ->
                    ( { model | flashMessage = Just "Failed to save file", flashIsError = True }, Cmd.none )

        GotDeleteResult val ->
            case Decode.decodeValue (Decode.field "success" Decode.bool) val of
                Ok True ->
                    ( { model | flashMessage = Just "File deleted", flashIsError = False, page = ResourcesPage }
                    , Cmd.batch [ Ports.listSnapshots (), Ports.listFiles (), Ports.requestStatus () ]
                    )

                _ ->
                    ( { model | flashMessage = Just "Failed to delete file", flashIsError = True }, Cmd.none )

        GotAddResult val ->
            case Decode.decodeValue (Decode.field "success" Decode.bool) val of
                Ok True ->
                    ( { model | flashMessage = Just "Staged successfully", flashIsError = False }
                    , Ports.requestStatus ()
                    )

                _ ->
                    ( { model | flashMessage = Just "Failed to stage", flashIsError = True }, Cmd.none )

        -- Commit
        ShowCommitDialog ->
            ( { model | showCommitDialog = True, commitMessage = "" }, Cmd.none )

        HideCommitDialog ->
            ( { model | showCommitDialog = False }, Cmd.none )

        UpdateCommitMessage m ->
            ( { model | commitMessage = m }, Cmd.none )

        DoCommit ->
            ( { model | showCommitDialog = False }
            , Ports.commitChanges model.commitMessage
            )

        GotCommitResult val ->
            case Decode.decodeValue (Decode.field "success" Decode.bool) val of
                Ok True ->
                    ( { model | flashMessage = Just "Committed successfully", flashIsError = False }
                    , Cmd.batch [ Ports.requestStatus (), Ports.listSnapshots (), Ports.listFiles () ]
                    )

                _ ->
                    let
                        detail =
                            val
                                |> Decode.decodeValue (Decode.field "error" Decode.string)
                                |> Result.withDefault "Commit failed"
                    in
                    ( { model | flashMessage = Just detail, flashIsError = True }, Cmd.none )

        -- Push
        DoPush ->
            ( model, Ports.pushChanges () )

        GotPushResult val ->
            case Decode.decodeValue (Decode.field "success" Decode.bool) val of
                Ok True ->
                    ( { model | flashMessage = Just "Pushed to remote", flashIsError = False }
                    , Ports.requestStatus ()
                    )

                _ ->
                    ( { model | flashMessage = Just "Push failed", flashIsError = True }, Cmd.none )

        -- Promote
        ShowPromoteDialog ->
            ( { model | showPromoteDialog = True, promoteChannel = model.status.channel }, Cmd.none )

        HidePromoteDialog ->
            ( { model | showPromoteDialog = False }, Cmd.none )

        UpdatePromoteChannel ch ->
            ( { model | promoteChannel = ch }, Cmd.none )

        DoPromote ->
            ( { model | showPromoteDialog = False }
            , Ports.promoteChanges model.promoteChannel
            )

        GotPromoteResult val ->
            case Decode.decodeValue (Decode.field "success" Decode.bool) val of
                Ok True ->
                    ( { model | flashMessage = Just "Promoted to main", flashIsError = False }
                    , Cmd.batch [ Ports.requestStatus (), Ports.requestChannels () ]
                    )

                _ ->
                    ( { model | flashMessage = Just "Promote failed", flashIsError = True }, Cmd.none )

        -- Channels
        ShowNewChannelDialog ->
            ( { model | showNewChannelDialog = True, newChannelName = "" }, Cmd.none )

        HideNewChannelDialog ->
            ( { model | showNewChannelDialog = False }, Cmd.none )

        UpdateNewChannelName n ->
            ( { model | newChannelName = n }, Cmd.none )

        DoCreateChannel ->
            ( { model | showNewChannelDialog = False }
            , Ports.createChannel model.newChannelName
            )

        GotCreateChannelResult val ->
            case Decode.decodeValue (Decode.field "success" Decode.bool) val of
                Ok True ->
                    ( { model | flashMessage = Just "Channel created", flashIsError = False }
                    , Cmd.batch [ Ports.requestChannels (), Ports.switchChannel model.newChannelName ]
                    )

                _ ->
                    ( { model | flashMessage = Just "Failed to create channel", flashIsError = True }, Cmd.none )

        DoSwitchChannel name ->
            ( model, Ports.switchChannel name )

        GotSwitchChannelResult val ->
            case Decode.decodeValue (Decode.field "success" Decode.bool) val of
                Ok True ->
                    ( { model | flashMessage = Just "Switched channel", flashIsError = False }
                    , Cmd.batch [ Ports.requestStatus (), Ports.requestChannels (), Ports.listSnapshots (), Ports.listFiles () ]
                    )

                _ ->
                    ( { model | flashMessage = Just "Failed to switch channel", flashIsError = True }, Cmd.none )

        -- History
        OpenHistory resourceId ->
            ( { model | historyResourceId = resourceId, historyEntries = [], page = HistoryPage }
            , Ports.requestHistory resourceId
            )

        GotHistoryResult val ->
            case Decode.decodeValue decodeHistoryResponse val of
                Ok entries ->
                    ( { model | historyEntries = entries }, Cmd.none )

                Err _ ->
                    ( model, Cmd.none )

        -- Log
        RefreshLog ->
            ( model, Ports.requestLog 50 )

        GotLogResult val ->
            case Decode.decodeValue (Decode.list decodeLogEntry) val of
                Ok entries ->
                    ( { model | logEntries = entries }, Cmd.none )

                Err _ ->
                    ( model, Cmd.none )

        -- Tooltip
        RequestTooltip resourceId ->
            ( { model | tooltipResourceId = resourceId, tooltipContent = "Loading..." }
            , Ports.getSnapshot resourceId
            )

        ClearTooltip ->
            ( { model | tooltipResourceId = "", tooltipContent = "" }, Cmd.none )

        GotSnapshotResult val ->
            let
                content =
                    val
                        |> Decode.decodeValue (Decode.field "content" Decode.string)
                        |> Result.withDefault ""

                error =
                    val
                        |> Decode.decodeValue (Decode.field "error" Decode.string)
                        |> Result.toMaybe
            in
            case error of
                Just _ ->
                    ( { model | tooltipContent = "(no snapshot)" }, Cmd.none )

                Nothing ->
                    ( { model | tooltipContent = content }, Cmd.none )

        -- Channel tooltip
        ShowChannelTooltip name ->
            ( { model | channelTooltipName = name }, Cmd.none )

        HideChannelTooltip ->
            ( { model | channelTooltipName = "" }, Cmd.none )

        -- Import
        GoToImport ->
            ( { model | page = ImportPage, importSourceChannel = "", importChannelResources = [], importChannelLog = [] }
            , Ports.requestChannels ()
            )

        UpdateImportChannel ch ->
            ( { model | importSourceChannel = ch }, Cmd.none )

        LoadImportChannel ->
            if String.isEmpty model.importSourceChannel then
                ( model, Cmd.none )
            else
                ( { model | importChannelResources = [], importChannelLog = [] }
                , Cmd.batch
                    [ Ports.listChannelResources model.importSourceChannel
                    , Ports.logForChannel model.importSourceChannel
                    ]
                )

        GotChannelResources val ->
            case Decode.decodeValue (Decode.list Decode.string) val of
                Ok ids ->
                    ( { model | importChannelResources = ids }, Cmd.none )

                Err _ ->
                    ( model, Cmd.none )

        GotChannelLog val ->
            case Decode.decodeValue (Decode.list decodeLogEntry) val of
                Ok entries ->
                    ( { model | importChannelLog = entries }, Cmd.none )

                Err _ ->
                    ( model, Cmd.none )

        ImportResourceFromChannel resourceId ->
            ( model
            , Ports.restoreFromChannel { resourceId = resourceId, channel = model.importSourceChannel }
            )

        ImportChangeset changeId ->
            ( model, Ports.cherryPick changeId )

        GotCherryPickResult val ->
            case Decode.decodeValue (Decode.field "success" Decode.bool) val of
                Ok True ->
                    ( { model | flashMessage = Just "Changeset imported (cherry-picked) successfully", flashIsError = False }
                    , Cmd.batch [ Ports.requestStatus (), Ports.listSnapshots (), Ports.listFiles () ]
                    )

                _ ->
                    let
                        detail =
                            val
                                |> Decode.decodeValue (Decode.field "error" Decode.string)
                                |> Result.withDefault "Cherry-pick failed"
                    in
                    ( { model | flashMessage = Just detail, flashIsError = True }, Cmd.none )

        -- Changeset detail
        ViewChangeset changeId ->
            ( { model | changesetDetail = Nothing, page = ChangesetDetailPage, expandedPatches = [] }
            , Ports.getChangeset changeId
            )

        GotChangesetResult val ->
            case Decode.decodeValue decodeChangesetDetail val of
                Ok detail ->
                    ( { model | changesetDetail = Just detail }, Cmd.none )

                Err _ ->
                    ( { model | flashMessage = Just "Failed to load changeset", flashIsError = True }, Cmd.none )

        ImportResourceFromChangeset resourceId changeId ->
            ( model
            , Ports.restoreFromChangeset { resourceId = resourceId, changeId = changeId }
            )

        -- Restore
        ShowRestoreDialog resourceId ->
            ( { model
                | showRestoreDialog = True
                , restoreResourceId = resourceId
                , restoreMode = "channel"
                , restoreChannel = ""
                , restoreChangeId = ""
              }
            , Cmd.none
            )

        HideRestoreDialog ->
            ( { model | showRestoreDialog = False }, Cmd.none )

        UpdateRestoreMode m ->
            ( { model | restoreMode = m }, Cmd.none )

        UpdateRestoreChannel ch ->
            ( { model | restoreChannel = ch }, Cmd.none )

        UpdateRestoreChangeId cid ->
            ( { model | restoreChangeId = cid }, Cmd.none )

        DoRestore ->
            let
                cmd =
                    if model.restoreMode == "channel" && not (String.isEmpty model.restoreChannel) then
                        Ports.restoreFromChannel { resourceId = model.restoreResourceId, channel = model.restoreChannel }
                    else if model.restoreMode == "changeset" && not (String.isEmpty model.restoreChangeId) then
                        Ports.restoreFromChangeset { resourceId = model.restoreResourceId, changeId = model.restoreChangeId }
                    else
                        Cmd.none
            in
            ( { model | showRestoreDialog = False }, cmd )

        GotRestoreResult val ->
            case Decode.decodeValue (Decode.field "success" Decode.bool) val of
                Ok True ->
                    ( { model | flashMessage = Just "Resource restored successfully", flashIsError = False }
                    , Cmd.batch [ Ports.listSnapshots (), Ports.listFiles (), Ports.requestStatus () ]
                    )

                _ ->
                    let
                        detail =
                            val
                                |> Decode.decodeValue (Decode.field "error" Decode.string)
                                |> Result.withDefault "Restore failed"
                    in
                    ( { model | flashMessage = Just detail, flashIsError = True }, Cmd.none )

        -- Notifications
        GotNotification json ->
            case Decode.decodeString decodeServerNotification json of
                Ok notif ->
                    let
                        newNotif =
                            notif model.nextNotifId

                        -- Add the notification's channel to remoteChannels if not already known
                        localNames =
                            List.map .name model.channels

                        updatedRemoteChannels =
                            if newNotif.channel /= "" && not (List.member newNotif.channel localNames) && not (List.member newNotif.channel model.remoteChannels) then
                                newNotif.channel :: model.remoteChannels
                            else
                                model.remoteChannels

                        -- Auto-sync main when notification targets it
                        syncCmd =
                            if newNotif.channel == "main" then
                                Ports.syncMain ()
                            else
                                Cmd.none
                    in
                    ( { model
                        | notifications = newNotif :: List.take 9 model.notifications
                        , nextNotifId = model.nextNotifId + 1
                        , notificationsVisible = True
                        , remoteChannels = updatedRemoteChannels
                      }
                    , Cmd.batch [ Ports.requestChannels (), syncCmd ]
                    )

                Err _ ->
                    ( model, Cmd.none )

        DismissNotification nid ->
            ( { model | notifications = List.filter (\n -> n.id /= nid) model.notifications }, Cmd.none )

        HideNotification nid ->
            ( { model
                | notifications =
                    List.map
                        (\n ->
                            if n.id == nid then
                                { n | hidden = True }
                            else
                                n
                        )
                        model.notifications
              }
            , Cmd.none
            )

        ToggleNotificationsVisible ->
            ( { model | notificationsVisible = not model.notificationsVisible }, Cmd.none )

        DismissAllNotifications ->
            ( { model | notifications = [] }, Cmd.none )

        PullNotifChannel channel ->
            ( { model
                | page = ImportPage
                , importSourceChannel = channel
                , importChannelResources = []
                , importChannelLog = []
              }
            , Cmd.batch
                [ Ports.requestChannels ()
                , Ports.listChannelResources channel
                , Ports.logForChannel channel
                ]
            )

        ImportNotifChangeset changeId _ ->
            ( model, Ports.cherryPick changeId )

        -- Remote channels
        SearchRemoteChannels ->
            ( model, Ports.listRemoteChannels () )

        GotRemoteChannels val ->
            -- Remote channels come as ChannelInfo objects with "name" field
            let
                nameDecoder =
                    Decode.list (Decode.field "name" Decode.string)
            in
            case Decode.decodeValue nameDecoder val of
                Ok chans ->
                    ( { model | remoteChannels = chans }, Cmd.none )

                Err _ ->
                    -- Fallback: try decoding as plain strings
                    case Decode.decodeValue (Decode.list Decode.string) val of
                        Ok chans ->
                            ( { model | remoteChannels = chans }, Cmd.none )

                        Err _ ->
                            ( model, Cmd.none )

        PullRemoteChannel channelName ->
            ( { model | flashMessage = Just ("Pulling channel '" ++ channelName ++ "' from server..."), flashIsError = False }
            , Ports.pullChannel channelName
            )

        GotPullChannelResult val ->
            case Decode.decodeValue (Decode.field "success" Decode.bool) val of
                Ok True ->
                    let
                        pulledChannel =
                            val
                                |> Decode.decodeValue (Decode.field "channel" Decode.string)
                                |> Result.withDefault ""

                        -- Remove from remoteChannels since it's now local
                        updatedRemote =
                            List.filter (\rn -> rn /= pulledChannel) model.remoteChannels
                    in
                    ( { model
                        | flashMessage = Just ("Channel '" ++ pulledChannel ++ "' pulled and switched successfully")
                        , flashIsError = False
                        , remoteChannels = updatedRemote
                      }
                    , Cmd.batch
                        [ Ports.requestStatus ()
                        , Ports.requestChannels ()
                        , Ports.listSnapshots ()
                        , Ports.listFiles ()
                        ]
                    )

                _ ->
                    let
                        detail =
                            val
                                |> Decode.decodeValue (Decode.field "error" Decode.string)
                                |> Result.withDefault "Failed to pull channel"
                    in
                    ( { model | flashMessage = Just detail, flashIsError = True }, Cmd.none )

        UpdateUserId uid ->
            ( { model | userId = uid }
            , if String.isEmpty uid then
                Cmd.none
              else
                Ports.setUser { name = uid, email = uid ++ "@dyna" }
            )

        -- Resource filter/sort
        UpdateResourceFilter f ->
            ( { model | resourceFilter = f }, Cmd.none )

        SetResourceSort s ->
            ( { model | resourceSort = s }, Cmd.none )

        -- Channel filter/sort
        UpdateChannelFilter f ->
            ( { model | channelFilter = f }, Cmd.none )

        SetChannelSort s ->
            ( { model | channelSort = s }, Cmd.none )

        ToggleShowAllChannels ->
            ( { model | showAllChannels = not model.showAllChannels }, Cmd.none )

        -- Stage All / Commit All
        StageAllWorking ->
            ( { model | flashMessage = Just "Staging all working files...", flashIsError = False }
            , Ports.stageAll ()
            )

        GotStageAllResult val ->
            case Decode.decodeValue (Decode.field "success" Decode.bool) val of
                Ok True ->
                    let
                        count =
                            val
                                |> Decode.decodeValue (Decode.field "count" Decode.int)
                                |> Result.withDefault 0
                    in
                    ( { model | flashMessage = Just ("Staged " ++ String.fromInt count ++ " file(s)"), flashIsError = False }
                    , Cmd.batch [ Ports.requestStatus (), Ports.listFiles () ]
                    )

                _ ->
                    let
                        detail =
                            val
                                |> Decode.decodeValue (Decode.field "error" Decode.string)
                                |> Result.withDefault "Stage all failed"
                    in
                    ( { model | flashMessage = Just detail, flashIsError = True }, Cmd.none )

        -- Unstage
        UnstageResource resourceId ->
            ( { model | flashMessage = Just ("Unstaging '" ++ resourceId ++ "'..."), flashIsError = False }
            , Ports.unstageResource resourceId
            )

        UnstageAllStaged ->
            ( { model | flashMessage = Just "Unstaging all staged changes...", flashIsError = False }
            , Ports.unstageAll ()
            )

        GotUnstageResult val ->
            case Decode.decodeValue (Decode.field "success" Decode.bool) val of
                Ok True ->
                    let
                        detail =
                            val
                                |> Decode.decodeValue (Decode.field "message" Decode.string)
                                |> Result.withDefault "Unstaged successfully"
                    in
                    ( { model | flashMessage = Just detail, flashIsError = False }
                    , Cmd.batch [ Ports.requestStatus (), Ports.listFiles () ]
                    )

                _ ->
                    let
                        detail =
                            val
                                |> Decode.decodeValue (Decode.field "error" Decode.string)
                                |> Result.withDefault "Unstage failed"
                    in
                    ( { model | flashMessage = Just detail, flashIsError = True }, Cmd.none )

        -- Sync main channel
        SyncMain ->
            ( model, Ports.syncMain () )

        GotSyncMainResult val ->
            case Decode.decodeValue (Decode.field "success" Decode.bool) val of
                Ok True ->
                    let
                        currentChannel =
                            model.status.channel

                        -- If user is currently on main, refresh everything
                        refreshCmd =
                            if currentChannel == "main" then
                                Cmd.batch
                                    [ Ports.requestStatus ()
                                    , Ports.listSnapshots ()
                                    , Ports.listFiles ()
                                    , Ports.requestChannels ()
                                    ]
                            else
                                Ports.requestChannels ()
                    in
                    ( { model | flashMessage = Just "Main channel synced from remote", flashIsError = False }
                    , refreshCmd
                    )

                _ ->
                    -- Silent failure for background sync
                    ( model, Cmd.none )

        -- Patch operations toggle
        TogglePatchOps targetResource ->
            let
                newExpanded =
                    if List.member targetResource model.expandedPatches then
                        List.filter (\r -> r /= targetResource) model.expandedPatches
                    else
                        targetResource :: model.expandedPatches
            in
            ( { model | expandedPatches = newExpanded }, Cmd.none )

        -- Collapsible sections
        ToggleStagedSection ->
            ( { model | stagedSectionOpen = not model.stagedSectionOpen }, Cmd.none )

        ToggleWorkingSection ->
            ( { model | workingSectionOpen = not model.workingSectionOpen }, Cmd.none )

        ToggleCommittedSection ->
            ( { model | committedSectionOpen = not model.committedSectionOpen }, Cmd.none )

        DismissFlash ->
            ( { model | flashMessage = Nothing }, Cmd.none )

        NoOp ->
            ( model, Cmd.none )



-- =========================================================================
-- DECODERS
-- =========================================================================


decodeStatus : Decode.Value -> Result Decode.Error StatusInfo
decodeStatus =
    Decode.decodeValue
        (Decode.map6 StatusInfo
            (Decode.field "channel" Decode.string)
            (Decode.field "staged" (Decode.list decodeStagedFile))
            (Decode.field "modified" (Decode.list Decode.string))
            (Decode.field "deleted" (Decode.list Decode.string))
            (Decode.field "unstaged_on_staged" (Decode.list Decode.string))
            (Decode.field "conflicts" (Decode.list Decode.string))
        )


decodeStagedFile : Decode.Decoder StagedFile
decodeStagedFile =
    Decode.map3 StagedFile
        (Decode.field "resource_id" Decode.string)
        (Decode.field "ops" Decode.int)
        (Decode.field "kind" Decode.string)


decodeChannelInfo : Decode.Decoder ChannelInfo
decodeChannelInfo =
    Decode.map4 ChannelInfo
        (Decode.field "name" Decode.string)
        (Decode.field "changeset_count" Decode.int)
        (Decode.field "head" (Decode.nullable Decode.string))
        (Decode.field "is_current" Decode.bool)


decodeLogEntry : Decode.Decoder LogEntry
decodeLogEntry =
    Decode.map7 LogEntry
        (Decode.field "change_id" Decode.string)
        (Decode.field "commit_hash" Decode.string)
        (Decode.field "message" Decode.string)
        (Decode.field "author" Decode.string)
        (Decode.field "created_at" Decode.string)
        (Decode.field "patch_count" Decode.int)
        (Decode.field "immutable" Decode.bool)


decodeHistoryResponse : Decode.Decoder (List HistoryEntry)
decodeHistoryResponse =
    Decode.field "entries"
        (Decode.list
            (Decode.map6 HistoryEntry
                (Decode.field "change_id" Decode.string)
                (Decode.field "commit_hash" Decode.string)
                (Decode.field "message" Decode.string)
                (Decode.field "author" Decode.string)
                (Decode.field "timestamp" Decode.string)
                (Decode.field "channel" Decode.string)
            )
        )


decodeChangesetDetail : Decode.Decoder ChangesetDetail
decodeChangesetDetail =
    Decode.map6 ChangesetDetail
        (Decode.field "change_id" Decode.string)
        (Decode.field "commit_hash" Decode.string)
        (Decode.field "message" Decode.string)
        (Decode.field "author" Decode.string)
        (Decode.field "created_at" Decode.string)
        (Decode.field "patches"
            (Decode.list
                (Decode.map4 PatchInfo
                    (Decode.field "target_resource" Decode.string)
                    (Decode.field "operations"
                        (Decode.list Decode.value |> Decode.map List.length)
                    )
                    (Decode.oneOf
                        [ Decode.field "result_snapshot" (Decode.nullable Decode.value)
                            |> Decode.map (\v -> v /= Nothing)
                        , Decode.succeed False
                        ]
                    )
                    (Decode.field "operations"
                        (Decode.list decodePatchOp)
                    )
                )
            )
        )


decodePatchOp : Decode.Decoder PatchOp
decodePatchOp =
    Decode.map4 PatchOp
        (Decode.field "op" Decode.string)
        (Decode.field "path" Decode.string)
        (Decode.maybe (Decode.field "value" (Decode.value |> Decode.map (Encode.encode 2))))
        (Decode.maybe (Decode.field "from" Decode.string))


decodeChangesetSummary : Decode.Decoder ChangesetSummary
decodeChangesetSummary =
    Decode.map5 ChangesetSummary
        (Decode.field "change_id" Decode.string)
        (Decode.field "message" Decode.string)
        (Decode.field "author" Decode.string)
        (Decode.field "patch_count" Decode.int)
        (Decode.field "affected_resources" (Decode.list Decode.string))


decodeServerNotification : Decode.Decoder (Int -> Notification)
decodeServerNotification =
    Decode.oneOf
        [ -- Push notification
          Decode.map4
            (\kind channel changesets timestamp id ->
                let
                    authorList =
                        changesets
                            |> List.map .author
                            |> List.filter (\a -> a /= "")
                            |> unique

                    authorStr =
                        if List.isEmpty authorList then
                            "someone"
                        else
                            String.join ", " authorList

                    title =
                        "Push to " ++ channel

                    body =
                        authorStr
                            ++ " pushed "
                            ++ String.fromInt (List.length changesets)
                            ++ " changeset(s)"
                in
                { kind = kind
                , channel = channel
                , title = title
                , body = body
                , changesets = changesets
                , id = id
                , dismissed = False
                , hidden = False
                }
            )
            (Decode.field "kind" Decode.string)
            (Decode.at [ "payload", "channel" ] Decode.string)
            (Decode.at [ "payload", "changesets" ] (Decode.list decodeChangesetSummary))
            (Decode.field "timestamp" Decode.string)
        , -- Promotion notification
          Decode.map4
            (\kind sourceChannel targetChannel changesets id ->
                let
                    title =
                        "Promotion: " ++ sourceChannel ++ " \u{2192} " ++ targetChannel

                    body =
                        String.fromInt (List.length changesets)
                            ++ " changeset(s) promoted"
                in
                { kind = kind
                , channel = targetChannel
                , title = title
                , body = body
                , changesets = changesets
                , id = id
                , dismissed = False
                , hidden = False
                }
            )
            (Decode.field "kind" Decode.string)
            (Decode.at [ "payload", "source_channel" ] Decode.string)
            (Decode.at [ "payload", "target_channel" ] Decode.string)
            (Decode.at [ "payload", "promoted_changesets" ] (Decode.list decodeChangesetSummary))
        , -- Fallback: generic notification
          Decode.map2
            (\kind timestamp id ->
                { kind = kind
                , channel = ""
                , title = kind
                , body = "New event at " ++ String.left 19 timestamp
                , changesets = []
                , id = id
                , dismissed = False
                , hidden = False
                }
            )
            (Decode.field "kind" Decode.string)
            (Decode.field "timestamp" Decode.string)
        ]



-- =========================================================================
-- VIEW
-- =========================================================================


view : Model -> Html Msg
view model =
    case model.page of
        SetupPage ->
            viewSetup model

        _ ->
            viewApp model


viewSetup : Model -> Html Msg
viewSetup model =
    div [ class "setup-screen" ]
        [ div [ class "setup-card" ]
            [ h2 [] [ span [ style "color" "#4a6cf7" ] [ text "Dyna" ] ]
            , p [] [ text "Connect to a Dyna server to start editing JSON resources collaboratively." ]
            , div [ class "form-group" ]
                [ label [] [ text "Server URL" ]
                , input
                    [ type_ "text"
                    , value model.serverUrl
                    , onInput UpdateServerUrl
                    , placeholder "http://localhost:8080"
                    ]
                    []
                ]
            , div [ style "display" "flex", style "gap" "8px" ]
                [ button [ class "btn btn-primary", onClick ConnectToServer ] [ text "New Repository" ]
                , button [ class "btn btn-ghost", onClick CloneFromServer ] [ text "Clone Existing" ]
                ]
            , viewFlash model
            ]
        ]


viewApp : Model -> Html Msg
viewApp model =
    div [ class "app-layout" ]
        [ viewHeader model
        , viewSidebar model
        , div [ class "main-content" ]
            [ viewFlash model
            , case model.page of
                ResourcesPage ->
                    viewResources model

                EditorPage ->
                    viewEditor model

                HistoryPage ->
                    viewHistory model

                LogPage ->
                    viewLog model

                ImportPage ->
                    viewImport model

                ChangesetDetailPage ->
                    viewChangesetDetail model

                _ ->
                    text ""
            ]
        , viewNotifications model
        , viewDialogs model
        ]


viewHeader : Model -> Html Msg
viewHeader model =
    let
        notifCount =
            List.length (List.filter (\n -> not n.hidden) model.notifications)
    in
    div [ class "app-header" ]
        [ h1 [] [ span [] [ text "Dyna" ], text " Editor" ]
        , div [ class "header-actions" ]
            [ span [ style "font-size" "12px", style "color" "#8b90a0" ]
                [ text ("Channel: " ++ model.status.channel) ]
            , button
                [ class "btn btn-sm btn-ghost"
                , onClick ShowCommitDialog
                , disabled (List.isEmpty model.status.staged)
                ]
                [ text "Commit" ]
            , button [ class "btn btn-sm btn-ghost", onClick DoPush ] [ text "Push" ]
            , button
                [ class "btn btn-sm btn-success"
                , onClick ShowPromoteDialog
                , disabled (model.status.channel == "main")
                ]
                [ text "Promote" ]
            , -- Notification bell
              div [ style "position" "relative", style "cursor" "pointer", onClick ToggleNotificationsVisible ]
                [ span [ style "font-size" "18px", style "color" "#8b90a0" ] [ text "\u{1F514}" ]
                , if notifCount > 0 then
                    span
                        [ style "position" "absolute"
                        , style "top" "-4px"
                        , style "right" "-6px"
                        , style "background" "#f87171"
                        , style "color" "white"
                        , style "font-size" "10px"
                        , style "font-weight" "700"
                        , style "padding" "1px 5px"
                        , style "border-radius" "999px"
                        , style "min-width" "16px"
                        , style "text-align" "center"
                        ]
                        [ text (String.fromInt notifCount) ]
                  else
                    text ""
                ]
            ]
        ]


viewSidebar : Model -> Html Msg
viewSidebar model =
    let
        maxChannelsToShow =
            5

        filteredChannels =
            if String.isEmpty model.channelFilter then
                model.channels
            else
                List.filter (\ch -> String.contains (String.toLower model.channelFilter) (String.toLower ch.name)) model.channels

        sortedChannels =
            case model.channelSort of
                ChannelSortAlpha ->
                    List.sortBy .name filteredChannels

                ChannelSortByChangesets ->
                    List.sortBy (\ch -> negate ch.changesetCount) filteredChannels

        visibleChannels =
            if model.showAllChannels then
                sortedChannels
            else
                List.take maxChannelsToShow sortedChannels

        hiddenCount =
            List.length sortedChannels - List.length visibleChannels

        -- Remote-only channels (not in local)
        localNames =
            List.map .name model.channels

        remoteOnlyChannels =
            model.remoteChannels
                |> List.filter (\rn -> not (List.member rn localNames))
                |> List.filter (\rn ->
                    if String.isEmpty model.channelFilter then
                        True
                    else
                        String.contains (String.toLower model.channelFilter) (String.toLower rn)
                )

        -- Working directory changes
        workingChanges =
            model.status.modified ++ model.status.deleted ++ model.status.unstagedOnStaged
    in
    div [ class "sidebar" ]
        [ div [ class "sidebar-section" ]
            [ h3 [] [ text "Navigation" ]
            , div
                [ class
                    (if model.page == ResourcesPage then
                        "sidebar-item active"
                     else
                        "sidebar-item"
                    )
                , onClick (GoToPage ResourcesPage)
                ]
                [ text "Resources" ]
            , div
                [ class
                    (if model.page == LogPage then
                        "sidebar-item active"
                     else
                        "sidebar-item"
                    )
                , onClick (GoToPage LogPage)
                ]
                [ text "Commit Log" ]
            , div
                [ class
                    (if model.page == ImportPage then
                        "sidebar-item active"
                     else
                        "sidebar-item"
                    )
                , onClick GoToImport
                ]
                [ text "Import" ]
            ]
        , div [ class "sidebar-section" ]
            [ div [ style "display" "flex", style "align-items" "center", style "justify-content" "space-between", style "padding" "0 8px", style "margin-bottom" "6px" ]
                [ h3 [ style "margin-bottom" "0" ] [ text "Channels" ]
                , div [ style "display" "flex", style "gap" "4px" ]
                    [ button
                        [ class "btn-icon"
                        , title "Sort alphabetically"
                        , onClick (SetChannelSort ChannelSortAlpha)
                        , style "opacity" (if model.channelSort == ChannelSortAlpha then "1" else "0.5")
                        ]
                        [ text "A\u{2193}" ]
                    , button
                        [ class "btn-icon"
                        , title "Sort by changesets"
                        , onClick (SetChannelSort ChannelSortByChangesets)
                        , style "opacity" (if model.channelSort == ChannelSortByChangesets then "1" else "0.5")
                        ]
                        [ text "#\u{2193}" ]
                    ]
                ]
            , div [ style "padding" "0 8px", style "margin-bottom" "6px" ]
                [ input
                    [ type_ "text"
                    , placeholder "Search channels..."
                    , value model.channelFilter
                    , onInput UpdateChannelFilter
                    , style "width" "100%"
                    , style "padding" "4px 8px"
                    , style "font-size" "11px"
                    , style "background" "var(--color-bg)"
                    , style "border" "1px solid var(--color-border)"
                    , style "border-radius" "4px"
                    , style "color" "var(--color-text)"
                    ]
                    []
                ]
            , div [ class "channel-list" ] (List.map (viewChannelItem model) visibleChannels)
            , if hiddenCount > 0 then
                div
                    [ class "sidebar-item"
                    , style "font-size" "11px"
                    , style "color" "var(--color-text-dim)"
                    , style "cursor" "pointer"
                    , onClick ToggleShowAllChannels
                    ]
                    [ text ("+ " ++ String.fromInt hiddenCount ++ " more channels") ]
              else if model.showAllChannels && List.length sortedChannels > maxChannelsToShow then
                div
                    [ class "sidebar-item"
                    , style "font-size" "11px"
                    , style "color" "var(--color-text-dim)"
                    , style "cursor" "pointer"
                    , onClick ToggleShowAllChannels
                    ]
                    [ text "Show less" ]
              else
                text ""
            , if not (List.isEmpty remoteOnlyChannels) then
                div []
                    [ div [ style "padding" "4px 8px", style "font-size" "10px", style "color" "var(--color-text-dim)", style "text-transform" "uppercase", style "letter-spacing" "0.08em", style "margin-top" "8px" ]
                        [ text "Remote only" ]
                    , div [] (List.map viewRemoteChannelItem remoteOnlyChannels)
                    ]
              else
                text ""
            , div [ style "display" "flex", style "gap" "4px", style "padding" "4px 8px" ]
                [ div [ class "sidebar-item", onClick ShowNewChannelDialog, style "flex" "1" ]
                    [ span [ style "color" "#4a6cf7" ] [ text "+ New" ] ]
                , div [ class "sidebar-item", onClick SearchRemoteChannels, style "flex" "1" ]
                    [ span [ style "color" "#60a5fa" ] [ text "Fetch" ] ]
                ]
            ]
        , div [ class "sidebar-section" ]
            [ h3 [] [ text "Staged Changes" ]
            , if List.isEmpty model.status.staged then
                div [ style "padding" "4px 8px", style "font-size" "12px", style "color" "#5a5f73" ]
                    [ text "No staged changes" ]
              else
                div [] (List.map viewStagedItem model.status.staged)
            ]
        , if not (List.isEmpty workingChanges) then
            div [ class "sidebar-section" ]
                [ h3 [] [ text "Working Directory" ]
                , div [] (List.map (viewWorkingDirItem model.status) (unique workingChanges))
                ]
          else
            text ""
        ]


viewChannelItem : Model -> ChannelInfo -> Html Msg
viewChannelItem model ch =
    let
        showTooltip =
            model.channelTooltipName == ch.name
    in
    div
        [ class
            (if ch.isCurrent then
                "sidebar-item active"
             else
                "sidebar-item"
            )
        , onClick (DoSwitchChannel ch.name)
        , onMouseEnter (ShowChannelTooltip ch.name)
        , onMouseLeave HideChannelTooltip
        , style "position" "relative"
        ]
        [ span
            [ class "dot"
            , class
                (if ch.name == "main" then
                    "main"
                 else if ch.isCurrent then
                    "current"
                 else
                    "branch"
                )
            ]
            []
        , span [ style "flex" "1", style "overflow" "hidden", style "text-overflow" "ellipsis", style "white-space" "nowrap" ]
            [ text ch.name ]
        , span [ style "font-size" "10px", style "color" "var(--color-text-dim)", style "margin-left" "auto", style "flex-shrink" "0" ]
            [ text (String.fromInt ch.changesetCount) ]
        , if ch.isCurrent then
            span [ style "margin-left" "4px", style "font-size" "10px", style "color" "#fbbf24" ] [ text "\u{25CF}" ]
          else
            text ""
        , if showTooltip then
            div [ class "channel-tooltip" ]
                [ div [ style "font-weight" "600", style "margin-bottom" "4px" ] [ text ch.name ]
                , div [ style "font-size" "11px", style "color" "var(--color-text-muted)" ]
                    [ text ("Changesets: " ++ String.fromInt ch.changesetCount) ]
                , div [ style "font-size" "11px", style "color" "var(--color-text-muted)" ]
                    [ text
                        ("Head: "
                            ++ (case ch.head of
                                    Just h ->
                                        String.left 8 h

                                    Nothing ->
                                        "none"
                               )
                        )
                    ]
                , if ch.isCurrent then
                    div [ style "font-size" "11px", style "color" "#fbbf24", style "margin-top" "2px" ] [ text "Current channel" ]
                  else
                    text ""
                ]
          else
            text ""
        ]


viewRemoteChannelItem : String -> Html Msg
viewRemoteChannelItem name =
    div
        [ class "sidebar-item"
        , style "font-size" "12px"
        , style "color" "var(--color-text-dim)"
        , style "cursor" "pointer"
        , onClick (PullRemoteChannel name)
        , title "Click to pull and switch to this channel"
        ]
        [ span [ class "dot", style "background" "#6366f1" ] []
        , span [ style "flex" "1", style "overflow" "hidden", style "text-overflow" "ellipsis", style "white-space" "nowrap" ]
            [ text name ]
        , span [ style "font-size" "10px", style "color" "var(--color-info)" ] [ text "remote" ]
        ]


viewWorkingDirItem : StatusInfo -> String -> Html Msg
viewWorkingDirItem status rid =
    let
        isModified =
            List.member rid status.modified

        isDeleted =
            List.member rid status.deleted

        label =
            if isDeleted then
                "deleted"
            else if isModified then
                "modified"
            else
                "unstaged"

        badgeClass =
            if isDeleted then
                "badge-deleted"
            else if isModified then
                "badge-modified"
            else
                "badge-staged"
    in
    div [ class "sidebar-item", style "font-size" "12px" ]
        [ span [ class "badge", class badgeClass ] [ text label ]
        , span [ style "margin-left" "6px", style "font-family" "var(--font-mono)", style "overflow" "hidden", style "text-overflow" "ellipsis", style "white-space" "nowrap" ]
            [ text (truncateId rid 20) ]
        ]


viewStagedItem : StagedFile -> Html Msg
viewStagedItem sf =
    div [ class "sidebar-item", style "font-size" "12px" ]
        [ span
            [ class "badge"
            , class
                (case sf.kind of
                    "new" ->
                        "badge-new"

                    "deleted" ->
                        "badge-deleted"

                    _ ->
                        "badge-modified"
                )
            ]
            [ text sf.kind ]
        , span [ style "margin-left" "6px", style "font-family" "var(--font-mono)" ]
            [ text (truncateId sf.resourceId 24) ]
        ]


viewResources : Model -> Html Msg
viewResources model =
    let
        -- Staged resources: resources that have been staged for commit
        stagedIds =
            List.map .resourceId model.status.staged

        -- Working directory resources: files that have local unstaged changes
        -- (new/untracked files not in snapshots, or modified files)
        -- Excludes anything that is already staged
        workingOnlyIds =
            model.resources
                |> List.filter (\rid -> not (List.member rid stagedIds))
                |> List.filter (\rid ->
                    not (List.member rid model.snapshots)
                        || List.member rid model.status.modified
                )

        -- Committed resources: resources in snapshots that are NOT staged
        -- and NOT modified (clean committed state)
        committedIds =
            model.snapshots
                |> List.filter (\rid -> not (List.member rid stagedIds))
                |> List.filter (\rid -> not (List.member rid model.status.modified))

        -- Deleted resources
        deletedIds =
            model.status.deleted

        -- Apply filter
        applyFilter ids =
            if String.isEmpty model.resourceFilter then
                ids
            else
                List.filter (\rid -> simpleMatch model.resourceFilter rid) ids

        -- Apply sort
        applySort ids =
            case model.resourceSort of
                SortByIdAsc ->
                    List.sort ids

                SortByIdDesc ->
                    List.reverse (List.sort ids)

        filteredStaged =
            applySort (applyFilter stagedIds)

        filteredWorking =
            applySort (applyFilter workingOnlyIds)

        filteredCommitted =
            applySort (applyFilter committedIds)

        filteredDeleted =
            applySort (applyFilter deletedIds)

        totalCount =
            List.length filteredStaged
                + List.length filteredWorking
                + List.length filteredCommitted
    in
    div []
        [ -- Header with filter and sort controls
          div [ class "panel" ]
            [ div [ class "panel-header" ]
                [ h2 [] [ text ("Resources (" ++ String.fromInt totalCount ++ ")") ]
                , div [ style "display" "flex", style "gap" "8px" ]
                    [ button [ class "btn btn-sm btn-ghost", onClick RefreshAll ] [ text "Refresh" ]
                    , button [ class "btn btn-sm btn-primary", onClick OpenNewResource ] [ text "+ New Resource" ]
                    ]
                ]
            , div [ style "padding" "12px 18px 0 18px", style "display" "flex", style "gap" "8px", style "align-items" "center" ]
                [ input
                    [ type_ "text"
                    , placeholder "Filter resources (regex on IDs)..."
                    , value model.resourceFilter
                    , onInput UpdateResourceFilter
                    , style "flex" "1"
                    , style "padding" "6px 10px"
                    , style "font-size" "12px"
                    , style "font-family" "var(--font-mono)"
                    ]
                    []
                , button
                    [ class "btn btn-sm btn-ghost"
                    , onClick (SetResourceSort SortByIdAsc)
                    , style "opacity" (if model.resourceSort == SortByIdAsc then "1" else "0.5")
                    ]
                    [ text "A\u{2191}" ]
                , button
                    [ class "btn btn-sm btn-ghost"
                    , onClick (SetResourceSort SortByIdDesc)
                    , style "opacity" (if model.resourceSort == SortByIdDesc then "1" else "0.5")
                    ]
                    [ text "A\u{2193}" ]
                ]
            ]

        -- Section 1: Staged Resources
        , if not (List.isEmpty filteredStaged) then
            div [ class "panel" ]
                [ div
                    [ class "panel-header"
                    , style "cursor" "pointer"
                    , onClick ToggleStagedSection
                    ]
                    [ div [ style "display" "flex", style "align-items" "center", style "gap" "8px" ]
                        [ span [ style "font-size" "12px", style "color" "#fbbf24" ]
                            [ text (if model.stagedSectionOpen then "\u{25BC}" else "\u{25B6}") ]
                        , h2 [ style "color" "#fbbf24", style "margin" "0" ]
                            [ text ("Staged (" ++ String.fromInt (List.length filteredStaged) ++ ")") ]
                        ]
                    , div [ style "display" "flex", style "gap" "8px" ]
                        [ button
                            [ class "btn btn-sm btn-primary"
                            , onClick ShowCommitDialog
                            ]
                            [ text "Commit All" ]
                        , button
                            [ class "btn btn-sm"
                            , style "background" "#f87171"
                            , style "color" "#fff"
                            , onClick UnstageAllStaged
                            ]
                            [ text "Unstage All" ]
                        ]
                    ]
                , if model.stagedSectionOpen then
                    div [ class "panel-body" ]
                        [ ul [ class "resource-list" ]
                            (List.map (viewStagedResourceItem model) filteredStaged)
                        ]
                  else
                    text ""
                ]
          else
            text ""

        -- Section 2: Working Directory
        , if not (List.isEmpty filteredWorking) || not (List.isEmpty filteredDeleted) then
            div [ class "panel" ]
                [ div
                    [ class "panel-header"
                    , style "cursor" "pointer"
                    , onClick ToggleWorkingSection
                    ]
                    [ div [ style "display" "flex", style "align-items" "center", style "gap" "8px" ]
                        [ span [ style "font-size" "12px", style "color" "#60a5fa" ]
                            [ text (if model.workingSectionOpen then "\u{25BC}" else "\u{25B6}") ]
                        , h2 [ style "color" "#60a5fa", style "margin" "0" ]
                            [ text ("Working Directory (" ++ String.fromInt (List.length filteredWorking + List.length filteredDeleted) ++ ")") ]
                        ]
                    , div [ style "display" "flex", style "gap" "8px" ]
                        [ button
                            [ class "btn btn-sm btn-primary"
                            , onClick StageAllWorking
                            ]
                            [ text "Stage All" ]
                        ]
                    ]
                , if model.workingSectionOpen then
                    div [ class "panel-body" ]
                        [ ul [ class "resource-list" ]
                            (List.map (viewWorkingResourceItem model) filteredWorking
                                ++ List.map viewDeletedItem filteredDeleted
                            )
                        ]
                  else
                    text ""
                ]
          else
            text ""

        -- Section 3: Committed Resources
        , if not (List.isEmpty filteredCommitted) then
            div [ class "panel" ]
                [ div
                    [ class "panel-header"
                    , style "cursor" "pointer"
                    , onClick ToggleCommittedSection
                    ]
                    [ div [ style "display" "flex", style "align-items" "center", style "gap" "8px" ]
                        [ span [ style "font-size" "12px", style "color" "#34d399" ]
                            [ text (if model.committedSectionOpen then "\u{25BC}" else "\u{25B6}") ]
                        , h2 [ style "color" "#34d399", style "margin" "0" ]
                            [ text ("Committed (" ++ String.fromInt (List.length filteredCommitted) ++ ")") ]
                        ]
                    ]
                , if model.committedSectionOpen then
                    div [ class "panel-body", style "max-height" "calc(100vh - 400px)", style "overflow-y" "auto" ]
                        [ ul [ class "resource-list" ]
                            (List.map (viewCommittedResourceItem model) filteredCommitted)
                        ]
                  else
                    text ""
                ]
          else
            text ""

        -- Empty state
        , if totalCount == 0 && List.isEmpty filteredDeleted then
            div [ class "panel" ]
                [ div [ class "panel-body" ]
                    [ div [ class "empty-state" ]
                        [ h3 [] [ text "No resources found" ]
                        , p []
                            [ text
                                (if String.isEmpty model.resourceFilter then
                                    "Create a new resource or clone from a remote server."
                                 else
                                    "No resources match the filter \"" ++ model.resourceFilter ++ "\"."
                                )
                            ]
                        ]
                    ]
                ]
          else
            text ""
        ]


-- | View a staged resource item with its staging kind badge
viewStagedResourceItem : Model -> String -> Html Msg
viewStagedResourceItem model resourceId =
    let
        stagedKind =
            model.status.staged
                |> List.filter (\s -> s.resourceId == resourceId)
                |> List.head
                |> Maybe.map .kind

        showTooltip =
            model.tooltipResourceId == resourceId && not (String.isEmpty model.tooltipContent)
    in
    li
        [ class "resource-item"
        , onMouseEnter (RequestTooltip resourceId)
        , onMouseLeave ClearTooltip
        , style "position" "relative"
        ]
        [ div [ style "display" "flex", style "align-items" "center", style "gap" "6px", style "flex" "1", style "min-width" "0" ]
            [ span [ class "resource-id", style "overflow" "hidden", style "text-overflow" "ellipsis", style "white-space" "nowrap" ] [ text resourceId ]
            , span
                [ class "badge"
                , class
                    (case stagedKind of
                        Just "new" ->
                            "badge-new"

                        Just "deleted" ->
                            "badge-deleted"

                        _ ->
                            "badge-staged"
                    )
                ]
                [ text
                    (case stagedKind of
                        Just k ->
                            k

                        Nothing ->
                            "staged"
                    )
                ]
            ]
        , div [ class "resource-actions" ]
            [ button [ class "btn btn-sm btn-ghost", onClick (OpenResource resourceId) ] [ text "Edit" ]
            , button [ class "btn btn-sm btn-ghost", style "color" "#f87171", onClick (UnstageResource resourceId) ] [ text "Unstage" ]
            , button [ class "btn btn-sm btn-ghost", onClick (OpenHistory resourceId) ] [ text "History" ]
            ]
        , if showTooltip then
            viewResourceTooltip model.tooltipContent
          else
            text ""
        ]


-- | View a working directory resource item (new or modified)
viewWorkingResourceItem : Model -> String -> Html Msg
viewWorkingResourceItem model resourceId =
    let
        isModified =
            List.member resourceId model.status.modified

        isNew =
            not (List.member resourceId model.snapshots)

        showTooltip =
            model.tooltipResourceId == resourceId && not (String.isEmpty model.tooltipContent)
    in
    li
        [ class "resource-item"
        , onMouseEnter (RequestTooltip resourceId)
        , onMouseLeave ClearTooltip
        , style "position" "relative"
        ]
        [ div [ style "display" "flex", style "align-items" "center", style "gap" "6px", style "flex" "1", style "min-width" "0" ]
            [ span [ class "resource-id", style "overflow" "hidden", style "text-overflow" "ellipsis", style "white-space" "nowrap" ] [ text resourceId ]
            , if isNew then
                span [ class "badge badge-new" ] [ text "new" ]
              else if isModified then
                span [ class "badge badge-modified" ] [ text "modified" ]
              else
                text ""
            ]
        , div [ class "resource-actions" ]
            [ button [ class "btn btn-sm btn-ghost", onClick (OpenResource resourceId) ] [ text "Edit" ]
            , button [ class "btn btn-sm btn-ghost", onClick (StageResource) ] [ text "Stage" ]
            , button [ class "btn btn-sm btn-ghost", onClick (ShowRestoreDialog resourceId) ] [ text "Restore" ]
            ]
        , if showTooltip then
            viewResourceTooltip model.tooltipContent
          else
            text ""
        ]


-- | View a committed (snapshot) resource item
viewCommittedResourceItem : Model -> String -> Html Msg
viewCommittedResourceItem model resourceId =
    let
        showTooltip =
            model.tooltipResourceId == resourceId && not (String.isEmpty model.tooltipContent)
    in
    li
        [ class "resource-item"
        , onMouseEnter (RequestTooltip resourceId)
        , onMouseLeave ClearTooltip
        , style "position" "relative"
        ]
        [ div [ style "display" "flex", style "align-items" "center", style "gap" "6px", style "flex" "1", style "min-width" "0" ]
            [ span [ class "resource-id", style "overflow" "hidden", style "text-overflow" "ellipsis", style "white-space" "nowrap" ] [ text resourceId ]
            ]
        , div [ class "resource-actions" ]
            [ button [ class "btn btn-sm btn-ghost", onClick (OpenResource resourceId) ] [ text "Edit" ]
            , button [ class "btn btn-sm btn-ghost", onClick (OpenHistory resourceId) ] [ text "History" ]
            , button [ class "btn btn-sm btn-ghost", onClick (ShowRestoreDialog resourceId) ] [ text "Restore" ]
            ]
        , if showTooltip then
            viewResourceTooltip model.tooltipContent
          else
            text ""
        ]


-- | Shared tooltip view for resource items
viewResourceTooltip : String -> Html Msg
viewResourceTooltip content =
    div [ class "resource-tooltip" ]
        [ pre [ style "margin" "0", style "white-space" "pre-wrap", style "word-break" "break-all" ]
            [ text (truncateContent content 500) ]
        ]


viewDeletedItem : String -> Html Msg
viewDeletedItem resourceId =
    li [ class "resource-item" ]
        [ span [ class "resource-id" ] [ text resourceId ]
        , div [ class "resource-actions" ]
            [ button [ class "btn btn-sm btn-danger", onClick StageDelete ] [ text "Stage Deletion" ]
            ]
        ]


viewEditor : Model -> Html Msg
viewEditor model =
    div []
        [ div [ class "panel" ]
            [ div [ class "panel-header" ]
                [ h2 []
                    [ text
                        (if model.editIsNew then
                            "New Resource"
                         else
                            "Edit Resource"
                        )
                    ]
                , div [ style "display" "flex", style "gap" "8px" ]
                    [ button [ class "btn btn-sm btn-ghost", onClick (GoToPage ResourcesPage) ] [ text "\u{2190} Back" ]
                    , if not model.editIsNew then
                        button [ class "btn btn-sm btn-ghost", onClick (OpenHistory model.editResourceId) ] [ text "History" ]
                      else
                        text ""
                    , if not model.editIsNew then
                        button [ class "btn btn-sm btn-ghost", onClick (ShowRestoreDialog model.editResourceId) ] [ text "Restore" ]
                      else
                        text ""
                    ]
                ]
            , div [ class "panel-body" ]
                [ div [ class "form-group" ]
                    [ label [] [ text "Resource ID (e.g. acme.entity.User)" ]
                    , input
                        [ type_ "text"
                        , value model.editResourceId
                        , onInput UpdateResourceId
                        , placeholder "acme.entity.User"
                        , disabled (not model.editIsNew)
                        , style "font-family" "var(--font-mono)"
                        ]
                        []
                    ]
                , div [ class "form-group" ]
                    [ label [] [ text "JSON Content" ]
                    , div [ class "json-editor" ]
                        [ textarea
                            [ value model.editContent
                            , onInput UpdateContent
                            , placeholder "{ }"
                            ]
                            []
                        , case model.editJsonError of
                            Just err ->
                                div [ class "json-error" ] [ text err ]

                            Nothing ->
                                text ""
                        ]
                    ]
                , div [ style "display" "flex", style "gap" "8px", style "flex-wrap" "wrap" ]
                    [ button
                        [ class "btn btn-primary"
                        , onClick SaveResource
                        , disabled (model.editJsonError /= Nothing || String.isEmpty model.editResourceId)
                        ]
                        [ text "Save to Working Directory" ]
                    , button
                        [ class "btn btn-success"
                        , onClick StageResource
                        , disabled (String.isEmpty model.editResourceId)
                        ]
                        [ text "Stage" ]
                    , if not model.editIsNew then
                        button [ class "btn btn-danger", onClick DeleteResource ] [ text "Delete" ]
                      else
                        text ""
                    ]
                , if model.status.channel == "main" then
                    div [ style "margin-top" "12px", style "padding" "10px 14px", style "background" "rgba(251,191,36,0.1)", style "border-radius" "8px", style "font-size" "12px", style "color" "#fbbf24" ]
                        [ text "\u{26A0} You are on the main channel. Edits should happen on a feature channel. "
                        , span [ style "text-decoration" "underline", style "cursor" "pointer", onClick ShowNewChannelDialog ] [ text "Create one" ]
                        ]
                  else
                    text ""
                ]
            ]
        ]


viewHistory : Model -> Html Msg
viewHistory model =
    div []
        [ div [ class "panel" ]
            [ div [ class "panel-header" ]
                [ h2 [] [ text ("History: " ++ model.historyResourceId) ]
                , div [ style "display" "flex", style "gap" "8px" ]
                    [ button [ class "btn btn-sm btn-ghost", onClick (GoToPage ResourcesPage) ] [ text "\u{2190} Back" ]
                    , button [ class "btn btn-sm btn-ghost", onClick (ShowRestoreDialog model.historyResourceId) ] [ text "Restore" ]
                    ]
                ]
            , div [ class "panel-body" ]
                [ if List.isEmpty model.historyEntries then
                    div [ class "empty-state" ]
                        [ h3 [] [ text "No history" ]
                        , p [] [ text "This resource has no recorded changes yet (local-only resources have no remote history)." ]
                        ]
                  else
                    div [ class "timeline" ]
                        (List.map (viewHistoryEntry model.historyResourceId) model.historyEntries)
                ]
            ]
        ]


viewHistoryEntry : String -> HistoryEntry -> Html Msg
viewHistoryEntry resourceId entry =
    div [ class "timeline-entry" ]
        [ div [ class "entry-header" ]
            [ span [ class "entry-hash" ] [ text (String.left 8 entry.commitHash) ]
            , span [ class "badge badge-staged" ] [ text entry.channel ]
            ]
        , div [ class "entry-message" ] [ text entry.message ]
        , div [ class "entry-meta" ]
            [ text (entry.author ++ " \u{00B7} " ++ String.left 19 entry.timestamp) ]
        , div [ style "margin-top" "6px" ]
            [ button
                [ class "btn btn-sm btn-ghost"
                , onClick (ImportResourceFromChangeset resourceId entry.changeId)
                ]
                [ text "Restore to this version" ]
            ]
        ]


viewLog : Model -> Html Msg
viewLog model =
    div []
        [ div [ class "panel" ]
            [ div [ class "panel-header" ]
                [ h2 [] [ text "Commit Log" ]
                , button [ class "btn btn-sm btn-ghost", onClick RefreshLog ] [ text "Refresh" ]
                ]
            , div [ class "panel-body" ]
                [ if List.isEmpty model.logEntries then
                    div [ class "empty-state" ]
                        [ h3 [] [ text "No commits yet" ]
                        , p [] [ text "Commit some changes to see them here." ]
                        ]
                  else
                    div [ class "timeline" ]
                        (List.map viewLogEntry model.logEntries)
                ]
            ]
        ]


viewLogEntry : LogEntry -> Html Msg
viewLogEntry entry =
    div [ class "timeline-entry" ]
        [ div [ class "entry-header" ]
            [ span [ class "entry-hash" ] [ text (String.left 8 entry.commitHash) ]
            , if entry.immutable then
                span [ class "badge badge-new" ] [ text "promoted" ]
              else
                text ""
            , span [ style "font-size" "11px", style "color" "#5a5f73" ]
                [ text (String.fromInt entry.patchCount ++ " patch(es)") ]
            ]
        , div [ class "entry-message" ] [ text entry.message ]
        , div [ class "entry-meta" ]
            [ span [ style "font-weight" "500", style "color" "#a78bfa" ] [ text entry.author ]
            , text (" \u{00B7} " ++ String.left 19 entry.createdAt)
            ]
        , div [ style "margin-top" "6px", style "display" "flex", style "gap" "6px" ]
            [ button [ class "btn btn-sm btn-ghost", onClick (ViewChangeset entry.changeId) ] [ text "Details" ]
            ]
        ]


viewImport : Model -> Html Msg
viewImport model =
    let
        otherChannels =
            List.filter (\ch -> not ch.isCurrent) model.channels
    in
    div []
        [ div [ class "panel" ]
            [ div [ class "panel-header" ]
                [ h2 [] [ text "Import from Channel" ]
                ]
            , div [ class "panel-body" ]
                [ div [ class "form-group" ]
                    [ label [] [ text "Source Channel" ]
                    , div [ style "display" "flex", style "gap" "8px" ]
                        [ select
                            [ onInput UpdateImportChannel
                            , style "flex" "1"
                            , style "padding" "8px 12px"
                            , style "background" "var(--color-bg)"
                            , style "border" "1px solid var(--color-border)"
                            , style "border-radius" "8px"
                            , style "color" "var(--color-text)"
                            , style "font-size" "13px"
                            ]
                            (option [ value "" ] [ text "Select a channel..." ]
                                :: List.map
                                    (\ch -> option [ value ch.name, selected (ch.name == model.importSourceChannel) ] [ text ch.name ])
                                    otherChannels
                            )
                        , button [ class "btn btn-sm btn-primary", onClick LoadImportChannel, disabled (String.isEmpty model.importSourceChannel) ] [ text "Load" ]
                        ]
                    ]
                , if not (List.isEmpty model.importChannelResources) then
                    div []
                        [ h3 [ style "margin-top" "16px", style "margin-bottom" "8px" ] [ text "Resources in channel" ]
                        , p [ style "font-size" "12px", style "color" "#8b90a0", style "margin-bottom" "8px" ]
                            [ text "Import a resource to copy its latest state into your working directory." ]
                        , ul [ class "resource-list" ]
                            (List.map viewImportResourceItem model.importChannelResources)
                        ]
                  else
                    text ""
                , if not (List.isEmpty model.importChannelLog) then
                    div []
                        [ h3 [ style "margin-top" "24px", style "margin-bottom" "8px" ] [ text "Changesets in channel" ]
                        , p [ style "font-size" "12px", style "color" "#8b90a0", style "margin-bottom" "8px" ]
                            [ text "Cherry-pick a changeset to import all its resource changes into your current channel." ]
                        , div [ class "timeline" ]
                            (List.map viewImportLogEntry model.importChannelLog)
                        ]
                  else
                    text ""
                ]
            ]
        ]


viewImportResourceItem : String -> Html Msg
viewImportResourceItem resourceId =
    li [ class "resource-item" ]
        [ span [ class "resource-id" ] [ text resourceId ]
        , div [ class "resource-actions" ]
            [ button [ class "btn btn-sm btn-primary", onClick (ImportResourceFromChannel resourceId) ] [ text "Import" ]
            ]
        ]


viewImportLogEntry : LogEntry -> Html Msg
viewImportLogEntry entry =
    div [ class "timeline-entry" ]
        [ div [ class "entry-header" ]
            [ span [ class "entry-hash" ] [ text (String.left 8 entry.commitHash) ]
            , span [ style "font-size" "11px", style "color" "#5a5f73" ]
                [ text (String.fromInt entry.patchCount ++ " patch(es)") ]
            ]
        , div [ class "entry-message" ] [ text entry.message ]
        , div [ class "entry-meta" ]
            [ span [ style "font-weight" "500", style "color" "#a78bfa" ] [ text entry.author ]
            , text (" \u{00B7} " ++ String.left 19 entry.createdAt)
            ]
        , div [ style "margin-top" "6px", style "display" "flex", style "gap" "6px" ]
            [ button [ class "btn btn-sm btn-primary", onClick (ImportChangeset entry.changeId) ] [ text "Cherry-pick" ]
            , button [ class "btn btn-sm btn-ghost", onClick (ViewChangeset entry.changeId) ] [ text "Details" ]
            ]
        ]


viewChangesetDetail : Model -> Html Msg
viewChangesetDetail model =
    div []
        [ div [ class "panel" ]
            [ div [ class "panel-header" ]
                [ h2 [] [ text "Changeset Details" ]
                , button [ class "btn btn-sm btn-ghost", onClick (GoToPage LogPage) ] [ text "\u{2190} Back" ]
                ]
            , div [ class "panel-body" ]
                [ case model.changesetDetail of
                    Nothing ->
                        div [ class "empty-state" ]
                            [ p [] [ text "Loading changeset..." ] ]

                    Just detail ->
                        div []
                            [ div [ style "margin-bottom" "16px" ]
                                [ div [ style "display" "flex", style "gap" "8px", style "align-items" "center", style "margin-bottom" "8px" ]
                                    [ span [ class "entry-hash" ] [ text (String.left 8 detail.commitHash) ]
                                    , span [ style "font-weight" "500", style "color" "#a78bfa" ] [ text detail.author ]
                                    , span [ style "font-size" "12px", style "color" "#5a5f73" ] [ text (String.left 19 detail.createdAt) ]
                                    ]
                                , div [ style "font-size" "15px", style "font-weight" "500", style "margin-bottom" "4px" ] [ text detail.message ]
                                , div [ style "font-size" "11px", style "color" "#5a5f73", style "font-family" "var(--font-mono)" ] [ text detail.changeId ]
                                ]
                            , h3 [ style "margin-bottom" "8px" ] [ text ("Patches (" ++ String.fromInt (List.length detail.patches) ++ ")") ]
                            , ul [ class "resource-list" ]
                                (List.map (viewPatchItem detail.changeId model.expandedPatches) detail.patches)
                            ]
                ]
            ]
        ]


viewPatchItem : String -> List String -> PatchInfo -> Html Msg
viewPatchItem changeId expandedPatches patch =
    let
        isExpanded =
            List.member patch.targetResource expandedPatches

        toggleIcon =
            if isExpanded then
                "\u{25BC}"
            else
                "\u{25B6}"
    in
    li [ style "margin-bottom" "4px" ]
        [ div [ class "resource-item" ]
            [ div [ style "cursor" "pointer", onClick (TogglePatchOps patch.targetResource) ]
                [ span [ style "margin-right" "6px", style "font-size" "10px", style "color" "#8b8fa3" ] [ text toggleIcon ]
                , span [ class "resource-id" ] [ text patch.targetResource ]
                , span [ style "margin-left" "8px", style "font-size" "11px", style "color" "#5a5f73" ]
                    [ text (String.fromInt patch.opsCount ++ " op(s)") ]
                ]
            , div [ class "resource-actions" ]
                [ button
                    [ class "btn btn-sm btn-primary"
                    , onClick (ImportResourceFromChangeset patch.targetResource changeId)
                    ]
                    [ text "Import this resource" ]
                ]
            ]
        , if isExpanded then
            div [ style "margin-left" "20px", style "margin-top" "4px", style "margin-bottom" "8px" ]
                (List.indexedMap viewPatchOp patch.operations)
          else
            text ""
        ]


viewPatchOp : Int -> PatchOp -> Html Msg
viewPatchOp idx op =
    let
        opColor =
            case op.op of
                "add" ->
                    "#34d399"

                "remove" ->
                    "#f87171"

                "replace" ->
                    "#fbbf24"

                "move" ->
                    "#60a5fa"

                "copy" ->
                    "#a78bfa"

                "test" ->
                    "#5a5f73"

                _ ->
                    "#8b8fa3"

        valueDisplay =
            case op.value of
                Just v ->
                    truncateContent v 200

                Nothing ->
                    ""

        fromDisplay =
            case op.from of
                Just f ->
                    " from: " ++ f

                Nothing ->
                    ""
    in
    div
        [ style "padding" "6px 10px"
        , style "margin-bottom" "2px"
        , style "background" "rgba(255,255,255,0.03)"
        , style "border-radius" "4px"
        , style "font-family" "var(--font-mono)"
        , style "font-size" "12px"
        , style "line-height" "1.5"
        ]
        [ div [ style "display" "flex", style "gap" "8px", style "align-items" "baseline" ]
            [ span
                [ style "font-weight" "600"
                , style "color" opColor
                , style "text-transform" "uppercase"
                , style "min-width" "60px"
                ]
                [ text op.op ]
            , span [ style "color" "#c4c9d4" ] [ text op.path ]
            , if fromDisplay /= "" then
                span [ style "color" "#8b8fa3" ] [ text fromDisplay ]
              else
                text ""
            ]
        , if valueDisplay /= "" then
            pre
                [ style "margin" "4px 0 0 68px"
                , style "padding" "6px 8px"
                , style "background" "rgba(0,0,0,0.2)"
                , style "border-radius" "3px"
                , style "font-size" "11px"
                , style "color" "#a0a4b0"
                , style "white-space" "pre-wrap"
                , style "word-break" "break-all"
                , style "max-height" "150px"
                , style "overflow-y" "auto"
                ]
                [ text valueDisplay ]
          else
            text ""
        ]


viewNotifications : Model -> Html Msg
viewNotifications model =
    let
        visibleNotifs =
            List.filter (\n -> not n.hidden) model.notifications
    in
    if not model.notificationsVisible || List.isEmpty visibleNotifs then
        text ""
    else
        div [ class "notifications" ]
            (div [ style "display" "flex", style "justify-content" "flex-end", style "margin-bottom" "4px", style "gap" "6px" ]
                [ button
                    [ class "btn btn-sm btn-ghost"
                    , style "font-size" "10px"
                    , onClick DismissAllNotifications
                    ]
                    [ text "Dismiss all" ]
                , button
                    [ class "btn btn-sm btn-ghost"
                    , style "font-size" "10px"
                    , onClick ToggleNotificationsVisible
                    ]
                    [ text "Hide" ]
                ]
                :: List.map viewNotificationToast visibleNotifs
            )


viewNotificationToast : Notification -> Html Msg
viewNotificationToast notif =
    div
        [ class ("notification-toast " ++ notif.kind) ]
        [ div [ style "display" "flex", style "justify-content" "space-between", style "align-items" "flex-start" ]
            [ div []
                [ div [ class "toast-title" ] [ text notif.title ]
                , div [ class "toast-body" ] [ text notif.body ]
                ]
            , div [ style "display" "flex", style "gap" "4px" ]
                [ span
                    [ style "cursor" "pointer"
                    , style "opacity" "0.6"
                    , style "font-size" "12px"
                    , style "padding" "0 4px"
                    , title "Hide"
                    , onClick (HideNotification notif.id)
                    ]
                    [ text "\u{2212}" ]
                , span
                    [ style "cursor" "pointer"
                    , style "opacity" "0.6"
                    , style "font-size" "14px"
                    , style "padding" "0 4px"
                    , title "Dismiss"
                    , onClick (DismissNotification notif.id)
                    ]
                    [ text "\u{2715}" ]
                ]
            ]
        , if not (List.isEmpty notif.changesets) then
            div [ style "margin-top" "8px", style "font-size" "11px" ]
                (List.map (viewNotifChangeset notif.channel) notif.changesets)
          else
            text ""
        , div [ style "margin-top" "8px", style "display" "flex", style "gap" "6px" ]
            [ if not (String.isEmpty notif.channel) then
                button
                    [ class "btn btn-sm btn-primary"
                    , onClick (PullNotifChannel notif.channel)
                    ]
                    [ text ("Import from " ++ notif.channel) ]
              else
                text ""
            ]
        ]


viewNotifChangeset : String -> ChangesetSummary -> Html Msg
viewNotifChangeset channel cs =
    div
        [ style "padding" "4px 0"
        , style "border-top" "1px solid rgba(255,255,255,0.08)"
        ]
        [ div [ style "display" "flex", style "gap" "6px", style "align-items" "center", style "flex-wrap" "wrap" ]
            [ span [ style "font-family" "var(--font-mono)", style "color" "#a78bfa" ]
                [ text (String.left 8 cs.changeId) ]
            , span [ style "font-weight" "500" ] [ text cs.message ]
            , span [ style "color" "#8b90a0" ]
                [ text ("by " ++ cs.author ++ " \u{00B7} " ++ String.fromInt cs.patchCount ++ " patch(es)") ]
            ]
        , if not (List.isEmpty cs.affectedResources) then
            div [ style "margin-top" "2px", style "color" "#8b90a0" ]
                [ text ("Resources: " ++ String.join ", " cs.affectedResources) ]
          else
            text ""
        , div [ style "margin-top" "4px" ]
            [ button
                [ class "btn btn-sm btn-ghost"
                , onClick (ImportNotifChangeset cs.changeId channel)
                ]
                [ text "Cherry-pick this changeset" ]
            ]
        ]


viewDialogs : Model -> Html Msg
viewDialogs model =
    div []
        [ if model.showCommitDialog then
            viewCommitDialog model
          else
            text ""
        , if model.showNewChannelDialog then
            viewNewChannelDialog model
          else
            text ""
        , if model.showPromoteDialog then
            viewPromoteDialog model
          else
            text ""
        , if model.showRestoreDialog then
            viewRestoreDialog model
          else
            text ""
        ]


viewCommitDialog : Model -> Html Msg
viewCommitDialog model =
    div [ class "modal-overlay", onClick HideCommitDialog ]
        [ div [ class "modal", stopPropagationOn "click" (Decode.succeed ( NoOp, True )) ]
            [ h3 [] [ text "Commit Changes" ]
            , div [ class "form-group" ]
                [ label [] [ text "User ID" ]
                , input
                    [ type_ "text"
                    , value model.userId
                    , onInput UpdateUserId
                    , placeholder "e.g. alice, bob"
                    , style "font-family" "var(--font-mono)"
                    ]
                    []
                ]
            , div [ class "form-group" ]
                [ label [] [ text "Commit Message" ]
                , input
                    [ type_ "text"
                    , value model.commitMessage
                    , onInput UpdateCommitMessage
                    , placeholder "Describe your changes..."
                    ]
                    []
                ]
            , div [ style "font-size" "12px", style "color" "#8b90a0", style "margin-bottom" "12px" ]
                [ text (String.fromInt (List.length model.status.staged) ++ " staged change(s)")
                , if not (String.isEmpty model.userId) then
                    text (" \u{00B7} Author: " ++ model.userId)
                  else
                    text " \u{00B7} Author: unknown"
                ]
            , div [ class "modal-actions" ]
                [ button [ class "btn btn-ghost", onClick HideCommitDialog ] [ text "Cancel" ]
                , button
                    [ class "btn btn-primary"
                    , onClick DoCommit
                    , disabled (String.isEmpty model.commitMessage)
                    ]
                    [ text "Commit" ]
                ]
            ]
        ]


viewNewChannelDialog : Model -> Html Msg
viewNewChannelDialog model =
    div [ class "modal-overlay", onClick HideNewChannelDialog ]
        [ div [ class "modal", stopPropagationOn "click" (Decode.succeed ( NoOp, True )) ]
            [ h3 [] [ text "Create Channel" ]
            , p [ style "font-size" "13px", style "color" "#8b90a0", style "margin-bottom" "12px" ]
                [ text "Edits happen on feature channels, never directly on main." ]
            , div [ class "form-group" ]
                [ label [] [ text "Channel Name" ]
                , input
                    [ type_ "text"
                    , value model.newChannelName
                    , onInput UpdateNewChannelName
                    , placeholder "feature/user-schema"
                    ]
                    []
                ]
            , div [ class "modal-actions" ]
                [ button [ class "btn btn-ghost", onClick HideNewChannelDialog ] [ text "Cancel" ]
                , button
                    [ class "btn btn-primary"
                    , onClick DoCreateChannel
                    , disabled (String.isEmpty model.newChannelName)
                    ]
                    [ text "Create & Switch" ]
                ]
            ]
        ]


viewPromoteDialog : Model -> Html Msg
viewPromoteDialog model =
    div [ class "modal-overlay", onClick HidePromoteDialog ]
        [ div [ class "modal", stopPropagationOn "click" (Decode.succeed ( NoOp, True )) ]
            [ h3 [] [ text "Promote to Main" ]
            , p [ style "font-size" "13px", style "color" "#8b90a0", style "margin-bottom" "12px" ]
                [ text ("Promote all changesets from \"" ++ model.promoteChannel ++ "\" to main.") ]
            , div [ class "modal-actions" ]
                [ button [ class "btn btn-ghost", onClick HidePromoteDialog ] [ text "Cancel" ]
                , button [ class "btn btn-success", onClick DoPromote ] [ text "Promote" ]
                ]
            ]
        ]


viewRestoreDialog : Model -> Html Msg
viewRestoreDialog model =
    let
        otherChannels =
            List.filter (\ch -> not ch.isCurrent) model.channels
    in
    div [ class "modal-overlay", onClick HideRestoreDialog ]
        [ div [ class "modal", stopPropagationOn "click" (Decode.succeed ( NoOp, True )) ]
            [ h3 [] [ text "Restore Resource" ]
            , p [ style "font-size" "13px", style "color" "#8b90a0", style "margin-bottom" "12px" ]
                [ text ("Restore \"" ++ model.restoreResourceId ++ "\" to a previous state.") ]
            , div [ class "form-group" ]
                [ label [] [ text "Restore from" ]
                , div [ style "display" "flex", style "gap" "12px", style "margin-bottom" "8px" ]
                    [ label [ style "display" "flex", style "align-items" "center", style "gap" "4px", style "cursor" "pointer", style "font-size" "13px" ]
                        [ input
                            [ type_ "radio"
                            , name "restoreMode"
                            , checked (model.restoreMode == "channel")
                            , onClick (UpdateRestoreMode "channel")
                            ]
                            []
                        , text "Another channel"
                        ]
                    , label [ style "display" "flex", style "align-items" "center", style "gap" "4px", style "cursor" "pointer", style "font-size" "13px" ]
                        [ input
                            [ type_ "radio"
                            , name "restoreMode"
                            , checked (model.restoreMode == "changeset")
                            , onClick (UpdateRestoreMode "changeset")
                            ]
                            []
                        , text "A specific changeset"
                        ]
                    ]
                ]
            , if model.restoreMode == "channel" then
                div [ class "form-group" ]
                    [ label [] [ text "Source Channel" ]
                    , select
                        [ onInput UpdateRestoreChannel
                        , style "width" "100%"
                        , style "padding" "8px 12px"
                        , style "background" "var(--color-bg)"
                        , style "border" "1px solid var(--color-border)"
                        , style "border-radius" "8px"
                        , style "color" "var(--color-text)"
                        , style "font-size" "13px"
                        ]
                        (option [ value "" ] [ text "Select channel..." ]
                            :: List.map
                                (\ch -> option [ value ch.name ] [ text ch.name ])
                                otherChannels
                        )
                    ]
              else
                div [ class "form-group" ]
                    [ label [] [ text "Changeset ID" ]
                    , input
                        [ type_ "text"
                        , value model.restoreChangeId
                        , onInput UpdateRestoreChangeId
                        , placeholder "Paste a change_id..."
                        , style "font-family" "var(--font-mono)"
                        , style "font-size" "12px"
                        ]
                        []
                    ]
            , div [ class "modal-actions" ]
                [ button [ class "btn btn-ghost", onClick HideRestoreDialog ] [ text "Cancel" ]
                , button
                    [ class "btn btn-primary"
                    , onClick DoRestore
                    , disabled
                        (if model.restoreMode == "channel" then
                            String.isEmpty model.restoreChannel
                         else
                            String.isEmpty model.restoreChangeId
                        )
                    ]
                    [ text "Restore" ]
                ]
            ]
        ]


viewFlash : Model -> Html Msg
viewFlash model =
    case model.flashMessage of
        Just msg ->
            div
                [ style "padding" "8px 14px"
                , style "border-radius" "8px"
                , style "font-size" "13px"
                , style "margin-bottom" "12px"
                , style "cursor" "pointer"
                , style "background"
                    (if model.flashIsError then
                        "rgba(248,113,113,0.12)"
                     else
                        "rgba(52,211,153,0.12)"
                    )
                , style "color"
                    (if model.flashIsError then
                        "#f87171"
                     else
                        "#34d399"
                    )
                , onClick DismissFlash
                ]
                [ text msg ]

        Nothing ->
            text ""



-- =========================================================================
-- HELPERS
-- =========================================================================


truncateId : String -> Int -> String
truncateId s maxLen =
    if String.length s > maxLen then
        String.left maxLen s ++ "\u{2026}"
    else
        s


truncateContent : String -> Int -> String
truncateContent s maxLen =
    if String.length s > maxLen then
        String.left maxLen s ++ "\n..."
    else
        s


unique : List String -> List String
unique list =
    List.foldl
        (\item acc ->
            if List.member item acc then
                acc
            else
                acc ++ [ item ]
        )
        []
        list


{-| Simple substring match for resource filtering.
Treats the filter as a case-insensitive substring match.
For more advanced regex, we'd need elm/regex, but substring covers most use cases.
-}
simpleMatch : String -> String -> Bool
simpleMatch pattern str =
    String.contains (String.toLower pattern) (String.toLower str)


filePathToResourceId : String -> String
filePathToResourceId path =
    path
        |> (\p -> if String.endsWith ".json" p then String.dropRight 5 p else p)
        |> String.replace "/" "."


negate : Int -> Int
negate n =
    -n



-- =========================================================================
-- SUBSCRIPTIONS
-- =========================================================================


subscriptions : Model -> Sub Msg
subscriptions _ =
    Sub.batch
        [ Ports.onInitResult GotInitResult
        , Ports.onCloneResult GotCloneResult
        , Ports.onWriteResult GotWriteResult
        , Ports.onReadResult GotReadResult
        , Ports.onDeleteResult GotDeleteResult
        , Ports.onAddResult GotAddResult
        , Ports.onStatusResult GotStatus
        , Ports.onCommitResult GotCommitResult
        , Ports.onPushResult GotPushResult
        , Ports.onPromoteResult GotPromoteResult
        , Ports.onChannelsResult GotChannels
        , Ports.onCreateChannelResult GotCreateChannelResult
        , Ports.onSwitchChannelResult GotSwitchChannelResult
        , Ports.onLogResult GotLogResult
        , Ports.onHistoryResult GotHistoryResult
        , Ports.onListFilesResult GotListFiles
        , Ports.onListSnapshotsResult GotListSnapshots
        , Ports.onNotification GotNotification
        , Ports.onSnapshotResult GotSnapshotResult
        , Ports.onCherryPickResult GotCherryPickResult
        , Ports.onRestoreResult GotRestoreResult
        , Ports.onChangesetResult GotChangesetResult
        , Ports.onChannelLogResult GotChannelLog
        , Ports.onChannelResourcesResult GotChannelResources
        , Ports.onRemoteChannelsResult GotRemoteChannels
        , Ports.onPullChannelResult GotPullChannelResult
        , Ports.onStageAllResult GotStageAllResult
        , Ports.onSyncMainResult GotSyncMainResult
        , Ports.onUnstageResult GotUnstageResult
        ]



-- =========================================================================
-- MAIN
-- =========================================================================


main : Program Decode.Value Model Msg
main =
    Browser.element
        { init = init
        , update = update
        , view = view
        , subscriptions = subscriptions
        }
