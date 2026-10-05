//! `GET /ws`: authenticates during the handshake, then runs one task per connection
//! that forwards hub events, handles JOIN_BOARD / LEAVE_BOARD / PONG, keeps a
//! heartbeat and closes the socket when the access token expires.
//!
//! The access token is offered as a WebSocket subprotocol
//! (`Sec-WebSocket-Protocol: zeroboard.v1, bearer.<jwt>`) so it never appears in
//! URLs, logs or browser history.

use std::sync::Arc;
use std::time::Duration;

use axum::extract::ws::{
    close_code, rejection::WebSocketUpgradeRejection, CloseFrame, Message, WebSocket,
    WebSocketUpgrade,
};
use axum::extract::State;
use axum::http::header::{HOST, ORIGIN, SEC_WEBSOCKET_PROTOCOL};
use axum::http::uri::Authority;
use axum::http::{HeaderMap, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use tokio::sync::mpsc;
use tokio::time::{interval_at, sleep_until, timeout, Instant, MissedTickBehavior};

use super::events::{self, ClientEvent, WsEvent};
use super::WsHub;
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

/// Subprotocol the client must offer; it is the one echoed back to the browser.
pub const WS_PROTOCOL: &str = "zeroboard.v1";
/// Offered alongside [`WS_PROTOCOL`] as `bearer.<jwt>`; never echoed back.
pub const BEARER_PROTOCOL_PREFIX: &str = "bearer.";
const MISSING_PROTOCOL_MESSAGE: &str = "websocket subprotocol zeroboard.v1 is required";
/// Application close code (4000–4999 range) telling the client to refresh its token
/// and reconnect.
pub const CLOSE_TOKEN_EXPIRED: u16 = 4401;
/// Generous for several tabs and devices; bounds per-user memory and task count.
const MAX_CONNECTIONS_PER_USER: usize = 10;
/// A peer that cannot absorb a frame within this time is treated as dead.
const SEND_TIMEOUT: Duration = Duration::from_secs(10);
/// Each JOIN_BOARD costs DB queries, so client messages are rate limited per connection.
const MESSAGE_WINDOW: Duration = Duration::from_secs(10);
const MAX_MESSAGES_PER_WINDOW: u32 = 30;

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

/// Every check runs before the upgrade, so failures are plain HTTP errors
/// (403 cross-origin, 401 bad token, 400 missing subprotocol, 429 too many connections).
pub async fn ws_connect(
    State(state): State<AppState>,
    headers: HeaderMap,
    upgrade: Result<WebSocketUpgrade, WebSocketUpgradeRejection>,
) -> Result<Response, AppError> {
    connect(state, headers, upgrade, HEARTBEAT).await
}

async fn connect(
    state: AppState,
    headers: HeaderMap,
    upgrade: Result<WebSocketUpgrade, WebSocketUpgradeRejection>,
    heartbeat: Heartbeat,
) -> Result<Response, AppError> {
    // Cross-site WebSocket hijacking: browsers always send Origin, and a page on
    // another site cannot forge it. Non-browser clients omit it but still need a token.
    if !is_same_origin(&headers) {
        tracing::warn!("ws handshake rejected: cross-origin request");
        return Err(AppError::Forbidden);
    }
    let offered = offered_protocols(&headers);
    let token = offered
        .iter()
        .find_map(|protocol| protocol.strip_prefix(BEARER_PROTOCOL_PREFIX))
        .filter(|token| !token.is_empty())
        .ok_or(AppError::Unauthorized)?;
    let claims = jwt::verify_access_token(token, &state.config.jwt_secret).map_err(|err| {
        tracing::debug!(error = %err, "ws access token rejected");
        AppError::Unauthorized
    })?;
    if !offered.contains(&WS_PROTOCOL) {
        return Err(AppError::BadRequest(MISSING_PROTOCOL_MESSAGE.to_string()));
    }
    let upgrade = match upgrade {
        Ok(upgrade) => upgrade,
        Err(rejection) => return Ok(rejection.into_response()),
    };

    // The slot is claimed here, before the 101 response, so the limit holds even
    // when many handshakes for the same user race.
    let user_id = claims.sub;
    let client_id = uuid::Uuid::new_v4().to_string();
    let (sender, outgoing) = mpsc::unbounded_channel();
    if !state
        .ws_hub
        .register(&client_id, &user_id, sender, MAX_CONNECTIONS_PER_USER)
    {
        tracing::warn!(%user_id, "ws handshake rejected: connection limit reached");
        return Err(AppError::TooManyRequests);
    }
    let registration = Registration {
        hub: state.ws_hub.clone(),
        client_id,
    };

    let expires_at = token_deadline(claims.exp);
    Ok(upgrade
        .protocols([WS_PROTOCOL])
        .max_message_size(MAX_CLIENT_MESSAGE_BYTES)
        .on_upgrade(move |socket| {
            run_client(
                state,
                socket,
                registration,
                outgoing,
                user_id,
                expires_at,
                heartbeat,
            )
        })
        .into_response())
}

/// Owns a hub slot from the handshake until the connection task ends. If the
/// upgrade never completes, axum drops the callback holding this, freeing the slot.
struct Registration {
    hub: Arc<WsHub>,
    client_id: String,
}

impl Drop for Registration {
    fn drop(&mut self) {
        self.hub.unregister(&self.client_id);
    }
}

fn offered_protocols(headers: &HeaderMap) -> Vec<&str> {
    headers
        .get_all(SEC_WEBSOCKET_PROTOCOL)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .filter(|protocol| !protocol.is_empty())
        .collect()
}

/// True when there is no Origin (non-browser client) or it names the same host
/// and port the request was sent to.
fn is_same_origin(headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get(ORIGIN) else {
        return true;
    };
    let origin = origin.to_str().ok().and_then(|raw| raw.parse::<Uri>().ok());
    let host = headers
        .get(HOST)
        .and_then(|raw| raw.to_str().ok())
        .and_then(|raw| raw.parse::<Authority>().ok());
    let (Some(origin), Some(host)) = (origin, host) else {
        return false;
    };
    let (Some(origin_authority), Some(default_port)) = (
        origin.authority(),
        origin.scheme_str().and_then(default_port),
    ) else {
        return false;
    };
    origin_authority.host().eq_ignore_ascii_case(host.host())
        && origin_authority.port_u16().unwrap_or(default_port)
            == host.port_u16().unwrap_or(default_port)
}

fn default_port(scheme: &str) -> Option<u16> {
    match scheme {
        "http" => Some(80),
        "https" => Some(443),
        _ => None,
    }
}

/// Maps the token's `exp` (Unix seconds) onto the monotonic clock.
fn token_deadline(exp: usize) -> Instant {
    let now = chrono::Utc::now().timestamp();
    let remaining = i64::try_from(exp).unwrap_or(i64::MAX).saturating_sub(now);
    Instant::now() + Duration::from_secs(u64::try_from(remaining).unwrap_or(0))
}

/// Fixed-window counter of client messages on one connection.
struct MessageBudget {
    window_start: Instant,
    used: u32,
}

impl MessageBudget {
    fn new() -> Self {
        Self {
            window_start: Instant::now(),
            used: 0,
        }
    }

    fn allow(&mut self, now: Instant) -> bool {
        if now.duration_since(self.window_start) >= MESSAGE_WINDOW {
            self.window_start = now;
            self.used = 0;
        }
        self.used += 1;
        self.used <= MAX_MESSAGES_PER_WINDOW
    }
}

fn close_frame(code: u16, reason: &'static str) -> Option<CloseFrame<'static>> {
    Some(CloseFrame {
        code,
        reason: reason.into(),
    })
}

async fn run_client(
    state: AppState,
    mut socket: WebSocket,
    registration: Registration,
    mut outgoing: mpsc::UnboundedReceiver<Message>,
    user_id: String,
    expires_at: Instant,
    heartbeat: Heartbeat,
) {
    let client_id = registration.client_id.as_str();
    tracing::info!(%client_id, %user_id, "ws client connected");

    let mut ping_timer = interval_at(Instant::now() + heartbeat.interval, heartbeat.interval);
    ping_timer.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut pong_deadline: Option<Instant> = None;
    let mut budget = MessageBudget::new();

    let close = loop {
        tokio::select! {
            incoming = socket.recv() => {
                let message = match incoming {
                    Some(Ok(Message::Close(_))) | None => break None,
                    Some(Ok(message)) => message,
                    Some(Err(err)) => {
                        tracing::debug!(error = %err, %client_id, "ws receive failed");
                        break None;
                    }
                };
                if matches!(message, Message::Text(_) | Message::Binary(_)) && !budget.allow(Instant::now()) {
                    tracing::warn!(%client_id, %user_id, "ws client exceeded message rate, closing");
                    break close_frame(close_code::POLICY, "rate limit exceeded");
                }
                if let Message::Text(text) = message {
                    handle_client_message(&state, client_id, &user_id, &text, &mut pong_deadline).await;
                }
            },
            message = outgoing.recv() => {
                let Some(message) = message else { break None };
                match timeout(SEND_TIMEOUT, socket.send(message)).await {
                    Ok(Ok(())) => {}
                    Ok(Err(err)) => {
                        tracing::debug!(error = %err, %client_id, "ws send failed");
                        break None;
                    }
                    Err(_) => {
                        tracing::warn!(%client_id, %user_id, "ws send timed out, closing");
                        break None;
                    }
                }
            },
            _ = ping_timer.tick() => {
                state.ws_hub.send_to_client(client_id, &events::ping());
                pong_deadline.get_or_insert(Instant::now() + heartbeat.timeout);
            },
            _ = sleep_until(pong_deadline.unwrap_or_else(Instant::now)), if pong_deadline.is_some() => {
                tracing::info!(%client_id, %user_id, "ws client missed heartbeat, closing");
                break None;
            },
            _ = sleep_until(expires_at) => {
                tracing::info!(%client_id, %user_id, "ws access token expired, closing");
                break close_frame(CLOSE_TOKEN_EXPIRED, "token expired");
            },
        }
    };

    disconnect(&state, client_id, &user_id);
    // Best-effort close frame; the peer may already be gone or stalled.
    let _ = timeout(SEND_TIMEOUT, socket.send(Message::Close(close))).await;
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

    use axum::http::{HeaderValue, StatusCode};
    use futures_util::{SinkExt, StreamExt};
    use serde_json::{json, Value};
    use tokio::net::TcpStream;
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    use tokio_tungstenite::tungstenite::handshake::client::Request as ClientRequest;
    use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
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
    /// Handshakes beyond the per-user limit in the concurrent limit test.
    const EXTRA_ATTEMPTS: usize = 5;
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
                     headers: HeaderMap,
                     upgrade: Result<WebSocketUpgrade, WebSocketUpgradeRejection>| async move {
                        connect(state, headers, upgrade, FAST_HEARTBEAT).await
                    },
                ),
            )
            .with_state(state)
    }

    /// Handshake request offering `protocols` (comma-joined), plus optional extra headers.
    fn handshake(addr: SocketAddr, protocols: &str, extra: &[(&str, &str)]) -> ClientRequest {
        let mut request = format!("ws://{addr}/ws").into_client_request().unwrap();
        if !protocols.is_empty() {
            request.headers_mut().insert(
                SEC_WEBSOCKET_PROTOCOL,
                HeaderValue::from_str(protocols).unwrap(),
            );
        }
        for (name, value) in extra {
            request.headers_mut().insert(
                axum::http::HeaderName::from_bytes(name.as_bytes()).unwrap(),
                HeaderValue::from_str(value).unwrap(),
            );
        }
        request
    }

    fn auth_protocols(token: &str) -> String {
        format!("{WS_PROTOCOL}, {BEARER_PROTOCOL_PREFIX}{token}")
    }

    async fn connect_client(addr: SocketAddr, user: &TestUser) -> Client {
        let (client, response) = connect_async(handshake(addr, &auth_protocols(&user.token), &[]))
            .await
            .unwrap();
        assert_eq!(response.headers()[SEC_WEBSOCKET_PROTOCOL], WS_PROTOCOL);
        client
    }

    async fn handshake_status(request: ClientRequest) -> StatusCode {
        match connect_async(request).await {
            Err(tungstenite::Error::Http(response)) => response.status(),
            Ok(_) => StatusCode::SWITCHING_PROTOCOLS,
            Err(other) => panic!("unexpected handshake error: {other:?}"),
        }
    }

    /// Reads until the server closes, returning the close code (if a frame was sent).
    async fn close_code_of(client: &mut Client) -> Option<CloseCode> {
        let closed = tokio::time::timeout(EVENT_TIMEOUT, async {
            while let Some(Ok(message)) = client.next().await {
                if let ClientMessage::Close(frame) = message {
                    return frame.map(|frame| frame.code);
                }
            }
            None
        })
        .await;
        closed.expect("server did not close the connection")
    }

    fn token_expiring_in(user: &TestUser, secret: &str, seconds: i64) -> String {
        let now = chrono::Utc::now().timestamp();
        let claims = jwt::Claims {
            sub: user.id.clone(),
            email: user.email.clone(),
            exp: usize::try_from(now + seconds).unwrap(),
            iat: usize::try_from(now).unwrap(),
        };
        jsonwebtoken::encode(
            &jsonwebtoken::Header::new(jsonwebtoken::Algorithm::HS256),
            &claims,
            &jsonwebtoken::EncodingKey::from_secret(secret.as_bytes()),
        )
        .unwrap()
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
        let user = t.user("u@example.com").await;
        let addr = serve(crate::router(t.state.clone())).await;
        let wrong_secret =
            token_expiring_in(&user, "another-secret-that-is-at-least-32-chars", 600);

        for protocols in [
            String::new(),
            WS_PROTOCOL.to_string(),
            format!("{WS_PROTOCOL}, {BEARER_PROTOCOL_PREFIX}"),
            auth_protocols("not-a-jwt"),
            auth_protocols(&wrong_secret),
        ] {
            assert_eq!(
                handshake_status(handshake(addr, &protocols, &[])).await,
                StatusCode::UNAUTHORIZED,
                "{protocols}"
            );
        }

        t.cleanup().await;
    }

    #[tokio::test]
    async fn token_in_query_string_is_not_accepted() {
        let t = TestApp::new().await;
        let user = t.user("u@example.com").await;
        let addr = serve(crate::router(t.state.clone())).await;

        let request = format!("ws://{addr}/ws?token={}", user.token)
            .into_client_request()
            .unwrap();
        assert_eq!(handshake_status(request).await, StatusCode::UNAUTHORIZED);

        t.cleanup().await;
    }

    #[tokio::test]
    async fn requires_the_zeroboard_subprotocol() {
        let t = TestApp::new().await;
        let user = t.user("u@example.com").await;
        let addr = serve(crate::router(t.state.clone())).await;

        let bearer_only = format!("{BEARER_PROTOCOL_PREFIX}{}", user.token);
        assert_eq!(
            handshake_status(handshake(addr, &bearer_only, &[])).await,
            StatusCode::BAD_REQUEST
        );

        t.cleanup().await;
    }

    #[tokio::test]
    async fn rejects_cross_origin_handshakes() {
        let t = TestApp::new().await;
        let user = t.user("u@example.com").await;
        let addr = serve(crate::router(t.state.clone())).await;
        let protocols = auth_protocols(&user.token);
        let same_origin = format!("http://{addr}");

        for origin in ["https://evil.example", "null", "http://127.0.0.1:1"] {
            assert_eq!(
                handshake_status(handshake(addr, &protocols, &[("origin", origin)])).await,
                StatusCode::FORBIDDEN,
                "{origin}"
            );
        }
        assert_eq!(
            handshake_status(handshake(addr, &protocols, &[("origin", &same_origin)])).await,
            StatusCode::SWITCHING_PROTOCOLS
        );

        t.cleanup().await;
    }

    #[test]
    fn same_origin_normalizes_default_ports_and_case() {
        let check = |origin: &str, host: &str| {
            let mut headers = HeaderMap::new();
            headers.insert(ORIGIN, HeaderValue::from_str(origin).unwrap());
            headers.insert(HOST, HeaderValue::from_str(host).unwrap());
            is_same_origin(&headers)
        };
        assert!(check("https://Board.Example.com", "board.example.com"));
        assert!(check("https://board.example.com", "board.example.com:443"));
        assert!(check("http://localhost:5173", "localhost:5173"));
        assert!(!check("http://localhost:5173", "localhost:3000"));
        assert!(!check(
            "https://board.example.com.evil.io",
            "board.example.com"
        ));
        assert!(!check("ftp://board.example.com", "board.example.com"));
        assert!(is_same_origin(&HeaderMap::new()));
    }

    #[tokio::test]
    async fn limits_connections_per_user() {
        let t = TestApp::new().await;
        let user = t.user("u@example.com").await;
        let addr = serve(crate::router(t.state.clone())).await;

        let protocols = auth_protocols(&user.token);

        // All handshakes race; exactly the limit may succeed.
        let attempts = (0..MAX_CONNECTIONS_PER_USER + EXTRA_ATTEMPTS)
            .map(|_| connect_async(handshake(addr, &protocols, &[])));
        let results = futures_util::future::join_all(attempts).await;
        let mut connected = Vec::new();
        let mut rejected = 0;
        for result in results {
            match result {
                Ok((client, _)) => connected.push(client),
                Err(tungstenite::Error::Http(response))
                    if response.status() == StatusCode::TOO_MANY_REQUESTS =>
                {
                    rejected += 1
                }
                Err(other) => panic!("unexpected handshake error: {other:?}"),
            }
        }
        assert_eq!(connected.len(), MAX_CONNECTIONS_PER_USER);
        assert_eq!(rejected, EXTRA_ATTEMPTS);

        // Closing one connection frees its slot.
        let mut closed = connected.pop().unwrap();
        closed.close(None).await.unwrap();
        let deadline = Instant::now() + EVENT_TIMEOUT;
        while handshake_status(handshake(addr, &protocols, &[])).await
            != StatusCode::SWITCHING_PROTOCOLS
        {
            assert!(Instant::now() < deadline, "slot was never freed");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }

        t.cleanup().await;
    }

    #[tokio::test]
    async fn dropping_an_unused_registration_frees_the_slot() {
        let t = TestApp::new().await;
        let hub = t.state.ws_hub.clone();
        let (sender, _outgoing) = mpsc::unbounded_channel();
        assert!(hub.register("c1", "u1", sender, 1));
        let registration = Registration {
            hub: hub.clone(),
            client_id: "c1".to_string(),
        };

        let (sender, _outgoing) = mpsc::unbounded_channel();
        assert!(!hub.register("c2", "u1", sender.clone(), 1));
        drop(registration);
        assert!(hub.register("c2", "u1", sender, 1));

        t.cleanup().await;
    }

    #[tokio::test]
    async fn closes_connection_when_token_expires() {
        let t = TestApp::new().await;
        let user = t.user("u@example.com").await;
        let addr = serve(crate::router(t.state.clone())).await;
        let token = token_expiring_in(&user, &t.state.config.jwt_secret, 1);

        let (mut client, _) = connect_async(handshake(addr, &auth_protocols(&token), &[]))
            .await
            .unwrap();
        assert_eq!(
            close_code_of(&mut client).await,
            Some(CloseCode::from(CLOSE_TOKEN_EXPIRED))
        );

        t.cleanup().await;
    }

    #[tokio::test]
    async fn closes_clients_that_flood_messages() {
        let t = TestApp::new().await;
        let user = t.user("u@example.com").await;
        let addr = serve(crate::router(t.state.clone())).await;
        let mut client = connect_client(addr, &user).await;

        for _ in 0..=MAX_MESSAGES_PER_WINDOW {
            // The server may close mid-flood; later sends failing is expected.
            let _ = client
                .send(ClientMessage::Text(r#"{"type":"PONG"}"#.into()))
                .await;
        }
        assert_eq!(close_code_of(&mut client).await, Some(CloseCode::Policy));

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
