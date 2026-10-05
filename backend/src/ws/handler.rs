//! `GET /ws?token=<jwt>`: authenticates, then runs one task per connection that
//! forwards hub events, handles JOIN_BOARD / LEAVE_BOARD / PONG and keeps a heartbeat.

use std::time::Duration;

use axum::extract::rejection::QueryRejection;
use axum::extract::ws::{
    rejection::WebSocketUpgradeRejection, Message, WebSocket, WebSocketUpgrade,
};
use axum::extract::{Query, State};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use serde::Deserialize;
use tokio::sync::mpsc;
use tokio::time::{interval_at, sleep_until, Instant, MissedTickBehavior};

use super::events::{self, ClientEvent, WsEvent};
use crate::auth::jwt;
use crate::errors::AppError;
use crate::handlers::parse_id;
use crate::models::WorkspaceRole;
use crate::services::access::require_board_role;
use crate::AppState;

const PING_INTERVAL: Duration = Duration::from_secs(30);
const PONG_TIMEOUT: Duration = Duration::from_secs(10);
/// Client → server events are tiny JSON objects; anything larger is abuse.
const MAX_CLIENT_MESSAGE_BYTES: usize = 4 * 1024;
const BOARD_ID_FIELD: &str = "board id";

#[derive(Debug, Clone, Copy)]
struct Heartbeat {
    interval: Duration,
    timeout: Duration,
}

const HEARTBEAT: Heartbeat = Heartbeat {
    interval: PING_INTERVAL,
    timeout: PONG_TIMEOUT,
};

pub fn router() -> Router<AppState> {
    Router::new().route("/ws", get(ws_connect))
}

#[derive(Deserialize)]
pub struct WsQuery {
    token: Option<String>,
}

/// Token is checked before the upgrade so a bad token is always a plain 401.
pub async fn ws_connect(
    State(state): State<AppState>,
    query: Result<Query<WsQuery>, QueryRejection>,
    upgrade: Result<WebSocketUpgrade, WebSocketUpgradeRejection>,
) -> Result<Response, AppError> {
    connect(state, query, upgrade, HEARTBEAT).await
}

async fn connect(
    state: AppState,
    query: Result<Query<WsQuery>, QueryRejection>,
    upgrade: Result<WebSocketUpgrade, WebSocketUpgradeRejection>,
    heartbeat: Heartbeat,
) -> Result<Response, AppError> {
    let token = query
        .ok()
        .and_then(|Query(query)| query.token)
        .ok_or(AppError::Unauthorized)?;
    let claims = jwt::verify_access_token(&token, &state.config.jwt_secret).map_err(|err| {
        tracing::debug!(error = %err, "ws access token rejected");
        AppError::Unauthorized
    })?;
    let upgrade = match upgrade {
        Ok(upgrade) => upgrade,
        Err(rejection) => return Ok(rejection.into_response()),
    };

    let user_id = claims.sub;
    Ok(upgrade
        .max_message_size(MAX_CLIENT_MESSAGE_BYTES)
        .on_upgrade(move |socket| run_client(state, socket, user_id, heartbeat))
        .into_response())
}

async fn run_client(state: AppState, mut socket: WebSocket, user_id: String, heartbeat: Heartbeat) {
    let client_id = uuid::Uuid::new_v4().to_string();
    let (sender, mut outgoing) = mpsc::unbounded_channel();
    state.ws_hub.register(&client_id, &user_id, sender);
    tracing::info!(%client_id, %user_id, "ws client connected");

    let mut ping_timer = interval_at(Instant::now() + heartbeat.interval, heartbeat.interval);
    ping_timer.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut pong_deadline: Option<Instant> = None;

    loop {
        tokio::select! {
            incoming = socket.recv() => match incoming {
                Some(Ok(Message::Text(text))) => {
                    handle_client_message(&state, &client_id, &user_id, &text, &mut pong_deadline).await;
                }
                Some(Ok(Message::Close(_))) | None => break,
                Some(Ok(_)) => {}
                Some(Err(err)) => {
                    tracing::debug!(error = %err, %client_id, "ws receive failed");
                    break;
                }
            },
            message = outgoing.recv() => {
                let Some(message) = message else { break };
                if let Err(err) = socket.send(message).await {
                    tracing::debug!(error = %err, %client_id, "ws send failed");
                    break;
                }
            },
            _ = ping_timer.tick() => {
                state.ws_hub.send_to_client(&client_id, &events::ping());
                pong_deadline.get_or_insert(Instant::now() + heartbeat.timeout);
            },
            _ = sleep_until(pong_deadline.unwrap_or_else(Instant::now)), if pong_deadline.is_some() => {
                tracing::info!(%client_id, %user_id, "ws client missed heartbeat, closing");
                break;
            },
        }
    }

    disconnect(&state, &client_id, &user_id);
    // Best-effort close frame; the peer may already be gone.
    let _ = socket.send(Message::Close(None)).await;
}

async fn handle_client_message(
    state: &AppState,
    client_id: &str,
    user_id: &str,
    text: &str,
    pong_deadline: &mut Option<Instant>,
) {
    let event = match serde_json::from_str::<ClientEvent>(text) {
        Ok(event) => event,
        Err(_) => {
            tracing::debug!(%client_id, "ignoring unrecognized ws message");
            return;
        }
    };
    match event {
        ClientEvent::Pong => *pong_deadline = None,
        ClientEvent::JoinBoard(board) => {
            join_board(state, client_id, user_id, &board.board_id).await
        }
        ClientEvent::LeaveBoard(board) => leave_board(state, client_id, &board.board_id),
    }
}

async fn join_board(state: &AppState, client_id: &str, user_id: &str, raw_board_id: &str) {
    let Ok(board_id) = parse_id(raw_board_id, BOARD_ID_FIELD) else {
        tracing::debug!(%client_id, "ws join rejected: invalid board id");
        return;
    };
    if let Err(err) = require_board_role(state, &board_id, user_id, WorkspaceRole::Viewer).await {
        tracing::info!(error = %err, %client_id, %user_id, %board_id, "ws join rejected");
        return;
    }

    let previous = state.ws_hub.join_board(client_id, &board_id);
    tracing::info!(%client_id, %user_id, %board_id, "ws client joined board");
    if let Some(previous) = previous.filter(|previous| *previous != board_id) {
        announce_departure(state, user_id, &previous);
    }
    broadcast_presence(state, &board_id);
}

fn leave_board(state: &AppState, client_id: &str, raw_board_id: &str) {
    let Ok(board_id) = parse_id(raw_board_id, BOARD_ID_FIELD) else {
        return;
    };
    // Ignore a stale LEAVE for a board the client already switched away from.
    if state.ws_hub.current_board(client_id).as_deref() != Some(board_id.as_str()) {
        return;
    }
    state.ws_hub.leave_board(client_id);
    tracing::info!(%client_id, %board_id, "ws client left board");
    broadcast_presence(state, &board_id);
}

fn disconnect(state: &AppState, client_id: &str, user_id: &str) {
    let board_id = state
        .ws_hub
        .unregister(client_id)
        .and_then(|client| client.board_id);
    tracing::info!(%client_id, %user_id, board_id = ?board_id, "ws client disconnected");
    if let Some(board_id) = board_id {
        announce_departure(state, user_id, &board_id);
    }
}

/// MEMBER_LEFT only once the user's last connection on the board is gone.
fn announce_departure(state: &AppState, user_id: &str, board_id: &str) {
    let active_users = state.ws_hub.active_users(board_id);
    if !active_users.iter().any(|active| active == user_id) {
        state
            .ws_hub
            .broadcast_to_board_all(board_id, &events::member_left(user_id, board_id));
    }
    state
        .ws_hub
        .broadcast_to_board_all(board_id, &events::presence_update(board_id, &active_users));
}

fn broadcast_presence(state: &AppState, board_id: &str) {
    let event: WsEvent = events::presence_update(board_id, &state.ws_hub.active_users(board_id));
    state.ws_hub.broadcast_to_board_all(board_id, &event);
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use axum::http::StatusCode;
    use futures_util::{SinkExt, StreamExt};
    use serde_json::{json, Value};
    use tokio::net::TcpStream;
    use tokio_tungstenite::tungstenite::{self, Message as ClientMessage};
    use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};

    use super::*;
    use crate::handlers::test_support::{BoardFixture, TestApp, TestUser};

    type Client = WebSocketStream<MaybeTlsStream<TcpStream>>;

    const EVENT_TIMEOUT: Duration = Duration::from_secs(5);
    /// How long a client must stay quiet to count as "received nothing".
    const SILENCE_WINDOW: Duration = Duration::from_millis(300);
    /// Same shape as production: the pong timeout is shorter than the ping interval.
    const FAST_HEARTBEAT: Heartbeat = Heartbeat {
        interval: Duration::from_millis(250),
        timeout: Duration::from_millis(100),
    };
    /// Several heartbeat intervals, comfortably past the pong timeout.
    const HEARTBEAT_OBSERVATION: Duration = Duration::from_millis(1100);

    async fn serve(app: Router) -> SocketAddr {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            .unwrap();
        });
        addr
    }

    fn fast_heartbeat_app(state: AppState) -> Router {
        Router::new()
            .route(
                "/ws",
                get(
                    |State(state): State<AppState>,
                     query: Result<Query<WsQuery>, QueryRejection>,
                     upgrade: Result<WebSocketUpgrade, WebSocketUpgradeRejection>| async move {
                        connect(state, query, upgrade, FAST_HEARTBEAT).await
                    },
                ),
            )
            .with_state(state)
    }

    async fn connect_client(addr: SocketAddr, user: &TestUser) -> Client {
        let (client, _) = connect_async(format!("ws://{addr}/ws?token={}", user.token))
            .await
            .unwrap();
        client
    }

    async fn send(client: &mut Client, event: Value) {
        client
            .send(ClientMessage::Text(event.to_string()))
            .await
            .unwrap();
    }

    async fn join(client: &mut Client, board_id: &str) {
        send(
            client,
            json!({ "type": "JOIN_BOARD", "payload": { "boardId": board_id } }),
        )
        .await;
    }

    /// Next event other than PING; panics on close or timeout.
    async fn next_event(client: &mut Client) -> Value {
        loop {
            let message = tokio::time::timeout(EVENT_TIMEOUT, client.next())
                .await
                .expect("timed out waiting for ws event")
                .expect("ws closed")
                .unwrap();
            if let ClientMessage::Text(text) = message {
                let event: Value = serde_json::from_str(&text).unwrap();
                if event["type"] != events::PING {
                    return event;
                }
            }
        }
    }

    async fn assert_silent(client: &mut Client) {
        if let Ok(Some(Ok(ClientMessage::Text(text)))) =
            tokio::time::timeout(SILENCE_WINDOW, client.next()).await
        {
            panic!("unexpected ws event: {text}");
        }
    }

    fn sorted(mut ids: Vec<String>) -> Vec<String> {
        ids.sort();
        ids
    }

    #[tokio::test]
    async fn rejects_missing_or_invalid_token_with_401() {
        let t = TestApp::new().await;
        let addr = serve(crate::router(t.state.clone())).await;

        for query in ["", "?token=", "?token=not-a-jwt"] {
            match connect_async(format!("ws://{addr}/ws{query}")).await {
                Err(tungstenite::Error::Http(response)) => {
                    assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{query}")
                }
                other => panic!("expected 401 for {query:?}, got {other:?}"),
            }
        }

        t.cleanup().await;
    }

    #[tokio::test]
    async fn board_clients_receive_mutations_and_presence() {
        let f = BoardFixture::new().await;
        let addr = serve(crate::router(f.t.state.clone())).await;

        let mut admin = connect_client(addr, &f.admin).await;
        join(&mut admin, &f.board_id).await;
        assert_eq!(
            next_event(&mut admin).await,
            json!({
                "type": "PRESENCE_UPDATE",
                "payload": { "boardId": f.board_id, "activeUsers": [f.admin.id] }
            })
        );

        let mut member = connect_client(addr, &f.member).await;
        join(&mut member, &f.board_id).await;
        let both = sorted(vec![f.admin.id.clone(), f.member.id.clone()]);
        for client in [&mut admin, &mut member] {
            let event = next_event(client).await;
            assert_eq!(event["type"], "PRESENCE_UPDATE");
            assert_eq!(event["payload"]["activeUsers"], json!(both));
        }

        let mut outsider = connect_client(addr, &f.outsider).await;
        join(&mut outsider, &f.board_id).await;
        join(&mut outsider, "not-a-uuid").await;

        let (status, list) =
            f.t.post(
                &format!("/api/boards/{}/lists", f.board_id),
                &f.admin,
                json!({ "name": "Todo" }),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED);
        for client in [&mut admin, &mut member] {
            let event = next_event(client).await;
            assert_eq!(event["type"], "LIST_CREATED");
            assert_eq!(event["payload"]["list"], list);
        }
        assert_silent(&mut outsider).await;

        let list_id = list["id"].as_str().unwrap();
        let (status, card) =
            f.t.post(
                &format!("/api/lists/{list_id}/cards"),
                &f.member,
                json!({ "title": "C" }),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED);
        let card_id = card["id"].as_str().unwrap();
        let event = next_event(&mut admin).await;
        assert_eq!(event["type"], "CARD_CREATED");
        assert_eq!(event["payload"]["card"], card);

        let (status, done) =
            f.t.post(
                &format!("/api/boards/{}/lists", f.board_id),
                &f.admin,
                json!({ "name": "Done" }),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(next_event(&mut admin).await["type"], "LIST_CREATED");
        let (status, _) =
            f.t.patch(
                &format!("/api/cards/{card_id}/move"),
                &f.member,
                json!({ "list_id": done["id"] }),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            next_event(&mut admin).await,
            json!({
                "type": "CARD_MOVED",
                "payload": {
                    "cardId": card_id,
                    "fromListId": list_id,
                    "toListId": done["id"],
                    "position": 1000.0
                }
            })
        );

        let (status, _) =
            f.t.delete(&format!("/api/cards/{card_id}"), &f.member)
                .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert_eq!(
            next_event(&mut admin).await,
            json!({ "type": "CARD_DELETED", "payload": { "cardId": card_id, "listId": done["id"] } })
        );

        let (status, _) = f.t.delete(&format!("/api/lists/{list_id}"), &f.admin).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert_eq!(
            next_event(&mut admin).await,
            json!({ "type": "LIST_DELETED", "payload": { "listId": list_id } })
        );
        assert_silent(&mut outsider).await;

        member.close(None).await.unwrap();
        assert_eq!(
            next_event(&mut admin).await,
            json!({ "type": "MEMBER_LEFT", "payload": { "userId": f.member.id, "boardId": f.board_id } })
        );
        assert_eq!(
            next_event(&mut admin).await["payload"]["activeUsers"],
            json!([f.admin.id])
        );

        f.t.cleanup().await;
    }

    #[tokio::test]
    async fn member_left_only_after_last_connection_closes() {
        let f = BoardFixture::new().await;
        let addr = serve(crate::router(f.t.state.clone())).await;
        let mut admin = connect_client(addr, &f.admin).await;
        join(&mut admin, &f.board_id).await;
        next_event(&mut admin).await;

        let mut tab1 = connect_client(addr, &f.member).await;
        let mut tab2 = connect_client(addr, &f.member).await;
        join(&mut tab1, &f.board_id).await;
        next_event(&mut admin).await;
        join(&mut tab2, &f.board_id).await;
        next_event(&mut admin).await;

        tab1.close(None).await.unwrap();
        let event = next_event(&mut admin).await;
        assert_eq!(event["type"], "PRESENCE_UPDATE");
        assert_eq!(
            event["payload"]["activeUsers"],
            json!(sorted(vec![f.admin.id.clone(), f.member.id.clone()]))
        );

        tab2.close(None).await.unwrap();
        assert_eq!(next_event(&mut admin).await["type"], "MEMBER_LEFT");

        f.t.cleanup().await;
    }

    #[tokio::test]
    async fn removed_member_stops_receiving_board_events() {
        let f = BoardFixture::new().await;
        let addr = serve(crate::router(f.t.state.clone())).await;
        let mut admin = connect_client(addr, &f.admin).await;
        join(&mut admin, &f.board_id).await;
        next_event(&mut admin).await;
        let mut member = connect_client(addr, &f.member).await;
        join(&mut member, &f.board_id).await;
        next_event(&mut admin).await;
        next_event(&mut member).await;

        let (status, _) =
            f.t.delete(
                &format!("/api/workspaces/{}/members/{}", f.workspace_id, f.member.id),
                &f.admin,
            )
            .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert_eq!(
            next_event(&mut admin).await,
            json!({ "type": "MEMBER_LEFT", "payload": { "userId": f.member.id, "boardId": f.board_id } })
        );

        join(&mut member, &f.board_id).await;
        let (status, _) =
            f.t.post(
                &format!("/api/boards/{}/lists", f.board_id),
                &f.admin,
                json!({ "name": "Secret" }),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(next_event(&mut admin).await["type"], "LIST_CREATED");
        assert_silent(&mut member).await;

        f.t.cleanup().await;
    }

    #[tokio::test]
    async fn invited_member_is_announced_to_board() {
        let f = BoardFixture::new().await;
        let addr = serve(crate::router(f.t.state.clone())).await;
        let newcomer = f.t.user("newcomer@example.com").await;
        let mut admin = connect_client(addr, &f.admin).await;
        join(&mut admin, &f.board_id).await;
        next_event(&mut admin).await;

        f.t.add_member(&f.workspace_id, &f.admin, &newcomer, "member")
            .await;
        let event = next_event(&mut admin).await;
        assert_eq!(event["type"], "MEMBER_JOINED");
        assert_eq!(event["payload"]["boardId"], f.board_id.as_str());
        assert_eq!(event["payload"]["user"]["user_id"], newcomer.id.as_str());

        f.t.cleanup().await;
    }

    #[tokio::test]
    async fn notifications_go_only_to_the_recipient() {
        let f = BoardFixture::new().await;
        let addr = serve(crate::router(f.t.state.clone())).await;
        let card_id = f.card("Ship it").await;
        let mut member = connect_client(addr, &f.member).await;
        let mut viewer = connect_client(addr, &f.viewer).await;

        f.assign(&card_id, &f.member.id).await;
        let event = next_event(&mut member).await;
        assert_eq!(event["type"], "NOTIFICATION");
        let notification = &event["payload"]["notification"];
        assert_eq!(notification["user_id"], f.member.id.as_str());
        assert_eq!(notification["type"], "assigned_to_card");
        assert_eq!(notification["read"], false);
        assert_silent(&mut viewer).await;

        f.t.cleanup().await;
    }

    #[tokio::test]
    async fn heartbeat_closes_clients_that_never_pong() {
        let t = TestApp::new().await;
        let user = t.user("idle@example.com").await;
        let addr = serve(fast_heartbeat_app(t.state.clone())).await;
        let mut client = connect_client(addr, &user).await;

        let mut pings = 0;
        let closed = tokio::time::timeout(EVENT_TIMEOUT, async {
            while let Some(Ok(message)) = client.next().await {
                match message {
                    ClientMessage::Text(text) if text == r#"{"type":"PING"}"# => pings += 1,
                    ClientMessage::Close(_) => break,
                    _ => {}
                }
            }
        })
        .await;
        assert!(
            closed.is_ok(),
            "server did not close an unresponsive client"
        );
        assert_eq!(pings, 1);

        t.cleanup().await;
    }

    #[tokio::test]
    async fn heartbeat_keeps_clients_that_pong() {
        let t = TestApp::new().await;
        let user = t.user("alive@example.com").await;
        let addr = serve(fast_heartbeat_app(t.state.clone())).await;
        let mut client = connect_client(addr, &user).await;

        let mut pings = 0;
        let deadline = Instant::now() + HEARTBEAT_OBSERVATION;
        while let Ok(message) = tokio::time::timeout_at(deadline, client.next()).await {
            match message.expect("server closed a responsive client").unwrap() {
                ClientMessage::Text(text) if text == r#"{"type":"PING"}"# => {
                    pings += 1;
                    send(&mut client, json!({ "type": "PONG" })).await;
                }
                ClientMessage::Close(_) => panic!("server closed a responsive client"),
                _ => {}
            }
        }
        assert!(pings >= 3, "expected several pings, got {pings}");

        t.cleanup().await;
    }
}
