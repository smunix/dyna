module Main exposing (main)

{-| lazy-elm-demo — Elm UI demo for the lazy-wasm Dyna client.

Shows connection status, known resources, resource details, and a live
update log with changeset metadata and materialised snapshots.
-}

import Browser
import Html exposing (..)
import Html.Attributes exposing (..)
import Html.Events exposing (..)
import Json.Decode as Decode exposing (Decoder)
import Json.Encode as Encode
import Ports



-- =========================================================================
-- MODEL
-- =========================================================================


type alias ChangesetInfo =
    { changeId : String
    , message : String
    , author : String
    , patchCount : Int
    , affectedResources : List String
    }


type alias UpdateEvent =
    { kind : String
    , timestamp : String
    , channel : String
    , changesets : List ChangesetInfo
    , newHead : Maybe String
    , affectedResourceIds : List String
    , updatedSnapshots : List ( String, Encode.Value )
    }


type alias Resource =
    { id : String
    , value : Encode.Value
    }


type ConnectionStatus
    = Disconnected
    | Connecting
    | Connected String


type alias Model =
    { serverUrl : String
    , channel : String
    , connectionStatus : ConnectionStatus
    , errorMessage : Maybe String
    , resourceIds : List String
    , resources : List Resource
    , selectedResource : Maybe Resource
    , updateLog : List UpdateEvent
    , streamedResources : List Resource
    , streamCount : Int
    }


init : () -> ( Model, Cmd Msg )
init _ =
    ( { serverUrl = "http://localhost:8080"
      , channel = "main"
      , connectionStatus = Disconnected
      , errorMessage = Nothing
      , resourceIds = []
      , resources = []
      , selectedResource = Nothing
      , updateLog = []
      , streamedResources = []
      , streamCount = 0
      }
    , Cmd.none
    )



-- =========================================================================
-- UPDATE
-- =========================================================================


type Msg
    = SetServerUrl String
    | SetChannel String
    | Connect
    | OnConnectResult Decode.Value
    | RequestList
    | OnResourceList Decode.Value
    | FetchResource String
    | OnResourceResult Decode.Value
    | FetchAll
    | OnAllResources Decode.Value
    | StreamAll
    | OnStreamedResource Decode.Value
    | OnStreamComplete Decode.Value
    | OnLiveUpdate Decode.Value
    | ClearLog


update : Msg -> Model -> ( Model, Cmd Msg )
update msg model =
    case msg of
        SetServerUrl url ->
            ( { model | serverUrl = url }, Cmd.none )

        SetChannel ch ->
            ( { model | channel = ch }, Cmd.none )

        Connect ->
            ( { model | connectionStatus = Connecting, errorMessage = Nothing }
            , Ports.connect { serverUrl = model.serverUrl, channel = model.channel }
            )

        OnConnectResult val ->
            case Decode.decodeValue connectResultDecoder val of
                Ok result ->
                    if result.success then
                        ( { model
                            | connectionStatus = Connected result.channel
                            , errorMessage = Nothing
                          }
                        , Ports.listResources ()
                        )

                    else
                        ( { model
                            | connectionStatus = Disconnected
                            , errorMessage = Just (Maybe.withDefault "Unknown error" result.error)
                          }
                        , Cmd.none
                        )

                Err e ->
                    ( { model
                        | connectionStatus = Disconnected
                        , errorMessage = Just (Decode.errorToString e)
                      }
                    , Cmd.none
                    )

        RequestList ->
            ( model, Ports.listResources () )

        OnResourceList val ->
            case Decode.decodeValue (Decode.list Decode.string) val of
                Ok ids ->
                    ( { model | resourceIds = ids }, Cmd.none )

                Err _ ->
                    ( model, Cmd.none )

        FetchResource rid ->
            ( model, Ports.getResource rid )

        OnResourceResult val ->
            case Decode.decodeValue resourceDecoder val of
                Ok res ->
                    ( { model | selectedResource = Just res }, Cmd.none )

                Err _ ->
                    ( model, Cmd.none )

        FetchAll ->
            ( model, Ports.getAllResources () )

        OnAllResources val ->
            case Decode.decodeValue allResourcesDecoder val of
                Ok resources ->
                    ( { model | resources = resources }, Cmd.none )

                Err _ ->
                    ( model, Cmd.none )

        StreamAll ->
            ( { model | streamedResources = [], streamCount = 0 }
            , Ports.streamAllResources ()
            )

        OnStreamedResource val ->
            case Decode.decodeValue resourceDecoder val of
                Ok res ->
                    ( { model
                        | streamedResources = model.streamedResources ++ [ res ]
                        , streamCount = model.streamCount + 1
                      }
                    , Cmd.none
                    )

                Err _ ->
                    ( model, Cmd.none )

        OnStreamComplete _ ->
            ( model, Cmd.none )

        OnLiveUpdate val ->
            case Decode.decodeValue updateEventDecoder val of
                Ok event ->
                    let
                        -- Add new resource IDs from the event
                        newIds =
                            List.filter (\rid -> not (List.member rid model.resourceIds))
                                event.affectedResourceIds

                        updatedIds =
                            model.resourceIds ++ newIds
                    in
                    ( { model
                        | updateLog = event :: model.updateLog
                        , resourceIds = updatedIds
                      }
                    , Cmd.none
                    )

                Err _ ->
                    ( model, Cmd.none )

        ClearLog ->
            ( { model | updateLog = [] }, Cmd.none )



-- =========================================================================
-- DECODERS
-- =========================================================================


type alias ConnectResult =
    { success : Bool
    , error : Maybe String
    , channel : String
    }


connectResultDecoder : Decoder ConnectResult
connectResultDecoder =
    Decode.map3 ConnectResult
        (Decode.field "success" Decode.bool)
        (Decode.maybe (Decode.field "error" Decode.string))
        (Decode.field "channel" Decode.string)


resourceDecoder : Decoder Resource
resourceDecoder =
    Decode.map2 Resource
        (Decode.field "id" Decode.string)
        (Decode.field "value" Decode.value)


allResourcesDecoder : Decoder (List Resource)
allResourcesDecoder =
    Decode.keyValuePairs Decode.value
        |> Decode.map (List.map (\( k, v ) -> Resource k v))


changesetInfoDecoder : Decoder ChangesetInfo
changesetInfoDecoder =
    Decode.map5 ChangesetInfo
        (Decode.field "change_id" Decode.string)
        (Decode.field "message" Decode.string)
        (Decode.field "author" Decode.string)
        (Decode.field "patch_count" Decode.int)
        (Decode.field "affected_resources" (Decode.list Decode.string))


updateEventDecoder : Decoder UpdateEvent
updateEventDecoder =
    Decode.map7 UpdateEvent
        (Decode.field "kind" Decode.string)
        (Decode.field "timestamp" Decode.string)
        (Decode.field "channel" Decode.string)
        (Decode.field "changesets" (Decode.list changesetInfoDecoder))
        (Decode.maybe (Decode.field "new_head" Decode.string))
        (Decode.field "affected_resource_ids" (Decode.list Decode.string))
        (Decode.field "updated_snapshots"
            (Decode.keyValuePairs Decode.value
                |> Decode.map identity
            )
        )



-- =========================================================================
-- SUBSCRIPTIONS
-- =========================================================================


subscriptions : Model -> Sub Msg
subscriptions _ =
    Sub.batch
        [ Ports.onConnectResult OnConnectResult
        , Ports.onResourceList OnResourceList
        , Ports.onResourceResult OnResourceResult
        , Ports.onAllResources OnAllResources
        , Ports.onStreamedResource OnStreamedResource
        , Ports.onStreamComplete OnStreamComplete
        , Ports.onLiveUpdate OnLiveUpdate
        ]



-- =========================================================================
-- VIEW
-- =========================================================================


view : Model -> Html Msg
view model =
    div [ class "app" ]
        [ viewHeader model
        , div [ class "main-content" ]
            [ div [ class "left-panel" ]
                [ viewConnectionForm model
                , viewResourceList model
                , viewActions model
                ]
            , div [ class "right-panel" ]
                [ viewResourceDetail model
                , viewUpdateLog model
                ]
            ]
        ]


viewHeader : Model -> Html Msg
viewHeader model =
    header [ class "app-header" ]
        [ h1 [] [ text "lazy-elm-demo" ]
        , span [ class "subtitle" ] [ text "Dyna lazy resource loader" ]
        , viewConnectionBadge model
        ]


viewConnectionBadge : Model -> Html Msg
viewConnectionBadge model =
    case model.connectionStatus of
        Disconnected ->
            span [ class "badge badge-disconnected" ] [ text "Disconnected" ]

        Connecting ->
            span [ class "badge badge-connecting" ] [ text "Connecting…" ]

        Connected ch ->
            span [ class "badge badge-connected" ]
                [ text ("Connected: " ++ ch) ]


viewConnectionForm : Model -> Html Msg
viewConnectionForm model =
    div [ class "card" ]
        [ h2 [] [ text "Connection" ]
        , div [ class "form-group" ]
            [ label [] [ text "Server URL" ]
            , input
                [ type_ "text"
                , value model.serverUrl
                , onInput SetServerUrl
                , placeholder "http://localhost:8080"
                ]
                []
            ]
        , div [ class "form-group" ]
            [ label [] [ text "Channel" ]
            , input
                [ type_ "text"
                , value model.channel
                , onInput SetChannel
                , placeholder "main"
                ]
                []
            ]
        , button
            [ onClick Connect
            , class "btn btn-primary"
            , disabled (model.connectionStatus == Connecting)
            ]
            [ text
                (case model.connectionStatus of
                    Connecting ->
                        "Connecting…"

                    _ ->
                        "Connect"
                )
            ]
        , case model.errorMessage of
            Just err ->
                div [ class "error-message" ] [ text err ]

            Nothing ->
                text ""
        ]


viewResourceList : Model -> Html Msg
viewResourceList model =
    div [ class "card" ]
        [ h2 []
            [ text "Known Resources"
            , span [ class "count" ]
                [ text ("(" ++ String.fromInt (List.length model.resourceIds) ++ ")") ]
            ]
        , if List.isEmpty model.resourceIds then
            p [ class "empty-state" ] [ text "No resources loaded yet." ]

          else
            ul [ class "resource-list" ]
                (List.map
                    (\rid ->
                        li [ onClick (FetchResource rid), class "resource-item" ]
                            [ span [ class "resource-icon" ] [ text "📄" ]
                            , text rid
                            ]
                    )
                    model.resourceIds
                )
        ]


viewActions : Model -> Html Msg
viewActions model =
    let
        isConnected =
            case model.connectionStatus of
                Connected _ ->
                    True

                _ ->
                    False
    in
    div [ class "card" ]
        [ h2 [] [ text "Actions" ]
        , div [ class "action-buttons" ]
            [ button
                [ onClick RequestList
                , class "btn"
                , disabled (not isConnected)
                ]
                [ text "Refresh List" ]
            , button
                [ onClick FetchAll
                , class "btn"
                , disabled (not isConnected)
                ]
                [ text "Fetch All" ]
            , button
                [ onClick StreamAll
                , class "btn"
                , disabled (not isConnected)
                ]
                [ text "Stream All" ]
            ]
        ]


viewResourceDetail : Model -> Html Msg
viewResourceDetail model =
    div [ class "card" ]
        [ h2 [] [ text "Resource Detail" ]
        , case model.selectedResource of
            Just res ->
                div []
                    [ h3 [ class "resource-id" ] [ text res.id ]
                    , div [ style "margin-top" "8px" ]
                        [ pre [ class "json-view" ]
                            [ text (jsonPretty res.value) ]
                        ]
                    ]

            Nothing ->
                p [ class "empty-state" ]
                    [ text "Click a resource to view its content." ]
        , if not (List.isEmpty model.resources) then
            div []
                [ h3 [] [ text "All Resources" ]
                , div [ class "all-resources" ]
                    (List.map viewResourceCard model.resources)
                ]

          else
            text ""
        , if not (List.isEmpty model.streamedResources) then
            div []
                [ h3 []
                    [ text "Streamed Resources"
                    , span [ class "count" ]
                        [ text ("(" ++ String.fromInt model.streamCount ++ ")") ]
                    ]
                , div [ class "all-resources" ]
                    (List.map viewResourceCard model.streamedResources)
                ]

          else
            text ""
        ]


viewResourceCard : Resource -> Html Msg
viewResourceCard res =
    div [ class "resource-card", onClick (FetchResource res.id) ]
        [ div [ class "resource-card-header" ] [ text res.id ]
        , let
            pretty = jsonPretty res.value
          in
          if String.length pretty > 2 then
            pre [ class "json-view json-view-small" ]
                [ text pretty ]
          else
            pre [ class "json-view json-view-small" ]
                [ text pretty ]
        ]


viewUpdateLog : Model -> Html Msg
viewUpdateLog model =
    div [ class "card update-log-card" ]
        [ div [ class "card-header-row" ]
            [ h2 [] [ text "Live Updates" ]
            , if not (List.isEmpty model.updateLog) then
                button [ onClick ClearLog, class "btn btn-small" ] [ text "Clear" ]

              else
                text ""
            ]
        , if List.isEmpty model.updateLog then
            p [ class "empty-state" ]
                [ text "Listening for live updates…" ]

          else
            div [ class "update-entries" ]
                (List.map viewUpdateEntry model.updateLog)
        ]


viewUpdateEntry : UpdateEvent -> Html Msg
viewUpdateEntry event =
    div [ class "update-entry" ]
        [ div [ class "update-header" ]
            [ span [ class "update-kind" ]
                [ text ("⚡ " ++ event.kind) ]
            , span [ class "update-channel" ]
                [ text event.channel ]
            , span [ class "update-time" ]
                [ text event.timestamp ]
            ]
        , case event.newHead of
            Just head ->
                div [ class "update-meta" ]
                    [ text ("New HEAD: " ++ head) ]

            Nothing ->
                text ""
        , div [ class "update-meta" ]
            [ text
                (String.fromInt (List.length event.affectedResourceIds)
                    ++ " resource(s) affected"
                )
            ]
        , div [ class "changesets" ]
            (List.indexedMap viewChangeset event.changesets)
        , if not (List.isEmpty event.updatedSnapshots) then
            div [ class "updated-snapshots" ]
                [ h4 [] [ text "Updated Snapshots" ]
                , div []
                    (List.map viewSnapshot event.updatedSnapshots)
                ]

          else
            text ""
        ]


viewChangeset : Int -> ChangesetInfo -> Html Msg
viewChangeset idx cs =
    div [ class "changeset" ]
        [ div [ class "changeset-header" ]
            [ text
                ("Changeset #"
                    ++ String.fromInt (idx + 1)
                    ++ " ["
                    ++ String.left 12 cs.changeId
                    ++ "]"
                )
            ]
        , div [ class "changeset-detail" ]
            [ div [] [ strong [] [ text "Author: " ], text cs.author ]
            , div [] [ strong [] [ text "Message: " ], text cs.message ]
            , div []
                [ strong [] [ text "Patches: " ]
                , text (String.fromInt cs.patchCount)
                ]
            , div []
                [ strong [] [ text "Resources: " ]
                , text (String.join ", " cs.affectedResources)
                ]
            ]
        ]


viewSnapshot : ( String, Encode.Value ) -> Html Msg
viewSnapshot ( rid, val ) =
    div [ class "snapshot-entry" ]
        [ div [ class "snapshot-id" ]
            [ span [] [ text "📄 " ]
            , text rid
            ]
        , pre [ class "json-view json-view-small" ]
            [ text (jsonPretty val) ]
        ]


strong : List (Attribute msg) -> List (Html msg) -> Html msg
strong attrs children =
    Html.node "strong" attrs children


jsonPretty : Encode.Value -> String
jsonPretty val =
    Encode.encode 2 val



-- =========================================================================
-- MAIN
-- =========================================================================


main : Program () Model Msg
main =
    Browser.element
        { init = init
        , update = update
        , subscriptions = subscriptions
        , view = view
        }
