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


type alias Notification =
    { kind : String
    , title : String
    , body : String
    , id : Int
    }


type alias Model =
    { page : Page
    , serverUrl : String
    , connected : Bool
    , status : StatusInfo
    , channels : List ChannelInfo
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

    -- Feedback
    , flashMessage : Maybe String
    , flashIsError : Bool
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
      , showCommitDialog = False
      , commitMessage = ""
      , showNewChannelDialog = False
      , newChannelName = ""
      , showPromoteDialog = False
      , promoteChannel = ""
      , notifications = []
      , nextNotifId = 0
      , flashMessage = Nothing
      , flashIsError = False
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
      -- Notifications
    | GotNotification String
    | DismissNotification Int
      -- User
    | UpdateUserId String
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
                        , Ports.connectNotifications ()
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
                        , Ports.connectNotifications ()
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
                    ( { model | resources = files }, Cmd.none )

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
                    -- Resource doesn't exist yet, allow creation
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
                    , Cmd.batch [ Ports.listSnapshots (), Ports.requestStatus () ]
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
                    , Cmd.batch [ Ports.requestStatus (), Ports.listSnapshots () ]
                    )

                _ ->
                    ( { model | flashMessage = Just "Commit failed", flashIsError = True }, Cmd.none )

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
                    , Cmd.batch [ Ports.requestStatus (), Ports.requestChannels (), Ports.listSnapshots () ]
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

        -- Notifications
        GotNotification json ->
            case Decode.decodeString decodeNotificationPayload json of
                Ok notif ->
                    let
                        newNotif =
                            { kind = notif.kind
                            , title = notif.title
                            , body = notif.body
                            , id = model.nextNotifId
                            }
                    in
                    ( { model
                        | notifications = newNotif :: List.take 4 model.notifications
                        , nextNotifId = model.nextNotifId + 1
                      }
                    , Cmd.none
                    )

                Err _ ->
                    ( model, Cmd.none )

        DismissNotification nid ->
            ( { model | notifications = List.filter (\n -> n.id /= nid) model.notifications }, Cmd.none )

        UpdateUserId uid ->
            ( { model | userId = uid }
            , if String.isEmpty uid then
                Cmd.none
              else
                Ports.setUser { name = uid, email = uid ++ "@dyna" }
            )

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


type alias NotificationPayload =
    { kind : String, title : String, body : String }


decodeNotificationPayload : Decode.Decoder NotificationPayload
decodeNotificationPayload =
    Decode.map3 NotificationPayload
        (Decode.field "event_type" Decode.string)
        (Decode.field "event_type" Decode.string)
        (Decode.oneOf
            [ Decode.field "message" Decode.string
            , Decode.field "details" Decode.string
            , Decode.succeed "New event"
            ]
        )



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

                _ ->
                    text ""
            ]
        , viewNotifications model
        , viewDialogs model
        ]


viewHeader : Model -> Html Msg
viewHeader model =
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
            ]
        ]


viewSidebar : Model -> Html Msg
viewSidebar model =
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
            ]
        , div [ class "sidebar-section" ]
            [ h3 [] [ text "Channels" ]
            , div [] (List.map (viewChannelItem model.status.channel) model.channels)
            , div [ class "sidebar-item", onClick ShowNewChannelDialog ]
                [ span [ style "color" "#4a6cf7" ] [ text "+ New Channel" ] ]
            ]
        , div [ class "sidebar-section" ]
            [ h3 [] [ text "Staged Changes" ]
            , if List.isEmpty model.status.staged then
                div [ style "padding" "4px 8px", style "font-size" "12px", style "color" "#5a5f73" ]
                    [ text "No staged changes" ]

              else
                div [] (List.map viewStagedItem model.status.staged)
            ]
        ]


viewChannelItem : String -> ChannelInfo -> Html Msg
viewChannelItem currentChannel ch =
    div
        [ class
            (if ch.name == currentChannel then
                "sidebar-item active"

             else
                "sidebar-item"
            )
        , onClick (DoSwitchChannel ch.name)
        ]
        [ span
            [ class "dot"
            , class
                (if ch.name == "main" then
                    "main"

                 else if ch.name == currentChannel then
                    "current"

                 else
                    "branch"
                )
            ]
            []
        , text ch.name
        , if ch.name == currentChannel then
            span [ style "margin-left" "auto", style "font-size" "10px", style "color" "#fbbf24" ] [ text "●" ]

          else
            text ""
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
        -- Build a unified list of all known resource IDs
        snapshotIds =
            model.snapshots

        stagedIds =
            List.map .resourceId model.status.staged

        modifiedIds =
            model.status.modified

        -- Deduplicate: start with snapshots, add staged/modified that aren't already there
        allIds =
            List.foldl
                (\rid acc ->
                    if List.member rid acc then
                        acc
                    else
                        acc ++ [ rid ]
                )
                snapshotIds
                (stagedIds ++ modifiedIds)
    in
    div []
        [ div [ class "panel" ]
            [ div [ class "panel-header" ]
                [ h2 [] [ text "Resources" ]
                , div [ style "display" "flex", style "gap" "8px" ]
                    [ button [ class "btn btn-sm btn-ghost", onClick RefreshAll ] [ text "Refresh" ]
                    , button [ class "btn btn-sm btn-primary", onClick OpenNewResource ] [ text "+ New Resource" ]
                    ]
                ]
            , div [ class "panel-body" ]
                [ if List.isEmpty allIds then
                    div [ class "empty-state" ]
                        [ h3 [] [ text "No resources yet" ]
                        , p [] [ text "Create a new resource or clone from a remote server." ]
                        ]

                  else
                    ul [ class "resource-list" ]
                        (List.map (viewResourceItem model.status) allIds)
                ]
            ]
        , if not (List.isEmpty model.status.deleted) then
            div [ class "panel" ]
                [ div [ class "panel-header" ] [ h2 [] [ text "Deleted (unstaged)" ] ]
                , div [ class "panel-body" ]
                    [ ul [ class "resource-list" ]
                        (List.map viewDeletedItem model.status.deleted)
                    ]
                ]

          else
            text ""
        ]


viewResourceItem : StatusInfo -> String -> Html Msg
viewResourceItem status resourceId =
    let
        isStaged =
            List.any (\s -> s.resourceId == resourceId) status.staged

        isModified =
            List.member resourceId status.modified

        stagedKind =
            status.staged
                |> List.filter (\s -> s.resourceId == resourceId)
                |> List.head
                |> Maybe.map .kind
    in
    li [ class "resource-item" ]
        [ div []
            [ span [ class "resource-id" ] [ text resourceId ]
            , if isStaged then
                span
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
                    , style "margin-left" "8px"
                    ]
                    [ text "staged" ]

              else if isModified then
                span [ class "badge badge-modified", style "margin-left" "8px" ] [ text "modified" ]

              else
                text ""
            ]
        , div [ class "resource-actions" ]
            [ button [ class "btn btn-sm btn-ghost", onClick (OpenResource resourceId) ] [ text "Edit" ]
            , button [ class "btn btn-sm btn-ghost", onClick (OpenHistory resourceId) ] [ text "History" ]
            ]
        ]


viewModifiedItem : String -> Html Msg
viewModifiedItem resourceId =
    li [ class "resource-item" ]
        [ span [ class "resource-id" ] [ text resourceId ]
        , div [ class "resource-actions" ]
            [ button [ class "btn btn-sm btn-primary", onClick (OpenResource resourceId) ] [ text "Edit & Stage" ]
            ]
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
                    [ button [ class "btn btn-sm btn-ghost", onClick (GoToPage ResourcesPage) ] [ text "← Back" ]
                    , if not model.editIsNew then
                        button [ class "btn btn-sm btn-ghost", onClick (OpenHistory model.editResourceId) ] [ text "History" ]

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
                        [ text "⚠ You are on the main channel. Edits should happen on a feature channel. "
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
                , button [ class "btn btn-sm btn-ghost", onClick (GoToPage ResourcesPage) ] [ text "← Back" ]
                ]
            , div [ class "panel-body" ]
                [ if List.isEmpty model.historyEntries then
                    div [ class "empty-state" ]
                        [ h3 [] [ text "No history" ]
                        , p [] [ text "This resource has no recorded changes yet." ]
                        ]

                  else
                    div [ class "timeline" ]
                        (List.map viewHistoryEntry model.historyEntries)
                ]
            ]
        ]


viewHistoryEntry : HistoryEntry -> Html Msg
viewHistoryEntry entry =
    div [ class "timeline-entry" ]
        [ div [ class "entry-header" ]
            [ span [ class "entry-hash" ] [ text (String.left 8 entry.commitHash) ]
            , span [ class "badge badge-staged" ] [ text entry.channel ]
            ]
        , div [ class "entry-message" ] [ text entry.message ]
        , div [ class "entry-meta" ]
            [ text (entry.author ++ " · " ++ String.left 19 entry.timestamp) ]
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
            [ text (entry.author ++ " · " ++ String.left 19 entry.createdAt) ]
        ]


viewNotifications : Model -> Html Msg
viewNotifications model =
    div [ class "notifications" ]
        (List.map viewNotificationToast model.notifications)


viewNotificationToast : Notification -> Html Msg
viewNotificationToast notif =
    div
        [ class ("notification-toast " ++ notif.kind)
        , onClick (DismissNotification notif.id)
        ]
        [ div [ class "toast-title" ] [ text notif.title ]
        , div [ class "toast-body" ] [ text notif.body ]
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
                    text (" · Author: " ++ model.userId)
                  else
                    text " · Author: unknown"
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
        String.left maxLen s ++ "…"

    else
        s



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
