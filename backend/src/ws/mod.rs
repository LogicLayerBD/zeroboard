//! Real-time hub: tracks connected clients and the board each one has joined,
//! and fans out [`WsEvent`]s. Sends never block; delivery failures are logged.

pub mod events;
mod handler;

use std::collections::{BTreeSet, HashMap};
use std::sync::{Mutex, MutexGuard, PoisonError};

use axum::extract::ws::Message;
use tokio::sync::mpsc;

pub use events::WsEvent;
pub use handler::router;

pub struct WsClient {
    user_id: String,
    board_id: Option<String>,
    sender: mpsc::UnboundedSender<Message>,
}

#[derive(Default)]
pub struct WsHub {
    /// client_id (uuid) -> client. Never held across an `.await`.
    clients: Mutex<HashMap<String, WsClient>>,
}

impl WsHub {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers the client unless `user_id` already has `max_per_user` connections.
    /// Counting and inserting share one lock so concurrent handshakes cannot overshoot.
    pub fn register(
        &self,
        client_id: &str,
        user_id: &str,
        sender: mpsc::UnboundedSender<Message>,
        max_per_user: usize,
    ) -> bool {
        let mut clients = self.lock();
        let existing = clients
            .values()
            .filter(|client| client.user_id == user_id)
            .count();
        if existing >= max_per_user {
            return false;
        }
        clients.insert(
            client_id.to_string(),
            WsClient {
                user_id: user_id.to_string(),
                board_id: None,
                sender,
            },
        );
        true
    }

    /// Removes the client, returning it so the caller can announce its departure.
    pub fn unregister(&self, client_id: &str) -> Option<WsClient> {
        self.lock().remove(client_id)
    }

    /// Moves the client onto `board_id`, returning the board it was on before.
    pub fn join_board(&self, client_id: &str, board_id: &str) -> Option<String> {
        let mut clients = self.lock();
        let client = clients.get_mut(client_id)?;
        client.board_id.replace(board_id.to_string())
    }

    /// Detaches the client from its board, returning the board it left.
    pub fn leave_board(&self, client_id: &str) -> Option<String> {
        self.lock().get_mut(client_id)?.board_id.take()
    }

    pub fn current_board(&self, client_id: &str) -> Option<String> {
        self.lock().get(client_id)?.board_id.clone()
    }

    /// Detaches every connection of `user_id` from `board_id` (e.g. after the user
    /// loses access) so they stop receiving its events. Returns how many were detached.
    pub fn remove_user_from_board(&self, user_id: &str, board_id: &str) -> usize {
        let mut detached = 0;
        for client in self.lock().values_mut() {
            if client.user_id == user_id && client.board_id.as_deref() == Some(board_id) {
                client.board_id = None;
                detached += 1;
            }
        }
        detached
    }

    /// Distinct users with at least one connection on the board, sorted.
    pub fn active_users(&self, board_id: &str) -> Vec<String> {
        let users: BTreeSet<String> = self
            .lock()
            .values()
            .filter(|client| client.board_id.as_deref() == Some(board_id))
            .map(|client| client.user_id.clone())
            .collect();
        users.into_iter().collect()
    }

    /// Sends to every client on the board except `exclude_client`.
    pub fn broadcast_to_board(
        &self,
        board_id: &str,
        event: &WsEvent,
        exclude_client: Option<&str>,
    ) {
        let Some(message) = encode(event) else {
            return;
        };
        let clients = self.lock();
        for (client_id, client) in clients.iter() {
            if client.board_id.as_deref() != Some(board_id)
                || Some(client_id.as_str()) == exclude_client
            {
                continue;
            }
            deliver(client_id, client, &message, &event.event_type);
        }
    }

    /// Sends to every client on the board, including the one that caused the event.
    pub fn broadcast_to_board_all(&self, board_id: &str, event: &WsEvent) {
        self.broadcast_to_board(board_id, event, None);
    }

    /// Sends to every connection of `user_id`, whatever board they are on.
    pub fn send_to_user(&self, user_id: &str, event: &WsEvent) {
        let Some(message) = encode(event) else {
            return;
        };
        let clients = self.lock();
        for (client_id, client) in clients.iter() {
            if client.user_id == user_id {
                deliver(client_id, client, &message, &event.event_type);
            }
        }
    }

    pub fn send_to_client(&self, client_id: &str, event: &WsEvent) {
        let Some(message) = encode(event) else {
            return;
        };
        if let Some(client) = self.lock().get(client_id) {
            deliver(client_id, client, &message, &event.event_type);
        }
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<String, WsClient>> {
        // The map stays consistent even if a holder panicked: every critical section
        // is a single insert/remove/field write.
        self.clients.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn encode(event: &WsEvent) -> Option<Message> {
    match serde_json::to_string(event) {
        Ok(text) => Some(Message::Text(text)),
        Err(err) => {
            tracing::error!(error = ?err, event_type = %event.event_type, "failed to encode ws event");
            None
        }
    }
}

/// A closed channel means the connection task already exited; it unregisters itself.
fn deliver(client_id: &str, client: &WsClient, message: &Message, event_type: &str) {
    if client.sender.send(message.clone()).is_err() {
        tracing::warn!(
            %client_id,
            user_id = %client.user_id,
            %event_type,
            "ws event dropped: client channel closed"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NO_LIMIT: usize = usize::MAX;

    fn connect(hub: &WsHub, client_id: &str, user_id: &str) -> mpsc::UnboundedReceiver<Message> {
        let (tx, rx) = mpsc::unbounded_channel();
        assert!(hub.register(client_id, user_id, tx, NO_LIMIT));
        rx
    }

    fn received_types(rx: &mut mpsc::UnboundedReceiver<Message>) -> Vec<String> {
        let mut types = Vec::new();
        while let Ok(Message::Text(text)) = rx.try_recv() {
            let value: serde_json::Value = serde_json::from_str(&text).unwrap();
            types.push(value["type"].as_str().unwrap().to_string());
        }
        types
    }

    #[test]
    fn broadcasts_only_to_clients_on_the_board() {
        let hub = WsHub::new();
        let mut a = connect(&hub, "a", "u1");
        let mut b = connect(&hub, "b", "u2");
        let mut c = connect(&hub, "c", "u3");
        hub.join_board("a", "board1");
        hub.join_board("b", "board1");
        hub.join_board("c", "board2");

        hub.broadcast_to_board("board1", &events::list_deleted("l1"), Some("a"));
        assert!(received_types(&mut a).is_empty());
        assert_eq!(received_types(&mut b), ["LIST_DELETED"]);
        assert!(received_types(&mut c).is_empty());

        hub.broadcast_to_board_all("board1", &events::list_deleted("l1"));
        assert_eq!(received_types(&mut a), ["LIST_DELETED"]);
        assert_eq!(received_types(&mut b), ["LIST_DELETED"]);
        assert!(received_types(&mut c).is_empty());
    }

    #[test]
    fn send_to_user_reaches_all_their_connections() {
        let hub = WsHub::new();
        let mut tab1 = connect(&hub, "t1", "u1");
        let mut tab2 = connect(&hub, "t2", "u1");
        let mut other = connect(&hub, "o", "u2");
        hub.join_board("t1", "board1");

        hub.send_to_user("u1", &events::ping());
        assert_eq!(received_types(&mut tab1), ["PING"]);
        assert_eq!(received_types(&mut tab2), ["PING"]);
        assert!(received_types(&mut other).is_empty());
    }

    #[test]
    fn register_enforces_per_user_limit() {
        const LIMIT: usize = 2;
        let hub = WsHub::new();
        let try_register = |client_id: &str, user_id: &str| {
            let (tx, _rx) = mpsc::unbounded_channel();
            hub.register(client_id, user_id, tx, LIMIT)
        };

        assert!(try_register("a", "u1"));
        assert!(try_register("b", "u1"));
        assert!(!try_register("c", "u1"));
        assert!(try_register("d", "u2"), "limit is per user");

        hub.unregister("a");
        assert!(try_register("c", "u1"), "slot is freed on unregister");
    }

    #[test]
    fn join_leave_and_unregister_track_board_membership() {
        let hub = WsHub::new();
        let _a = connect(&hub, "a", "u1");
        let _b = connect(&hub, "b", "u1");
        let _c = connect(&hub, "c", "u2");

        assert_eq!(hub.join_board("a", "board1"), None);
        assert_eq!(hub.join_board("a", "board2"), Some("board1".to_string()));
        hub.join_board("b", "board2");
        hub.join_board("c", "board2");
        assert_eq!(hub.active_users("board2"), ["u1", "u2"]);
        assert!(hub.active_users("board1").is_empty());

        assert_eq!(hub.leave_board("b"), Some("board2".to_string()));
        assert_eq!(hub.leave_board("b"), None);
        assert_eq!(hub.active_users("board2"), ["u1", "u2"]);

        let removed = hub.unregister("a").unwrap();
        assert_eq!(removed.board_id.as_deref(), Some("board2"));
        assert_eq!(hub.active_users("board2"), ["u2"]);
        assert!(hub.unregister("a").is_none());
        assert_eq!(hub.join_board("a", "board1"), None);
    }

    #[test]
    fn remove_user_from_board_stops_delivery() {
        let hub = WsHub::new();
        let mut a = connect(&hub, "a", "u1");
        hub.join_board("a", "board1");

        assert_eq!(hub.remove_user_from_board("u1", "board1"), 1);
        hub.broadcast_to_board_all("board1", &events::list_deleted("l1"));
        assert!(received_types(&mut a).is_empty());
        assert_eq!(hub.current_board("a"), None);
    }

    #[test]
    fn closed_channels_do_not_break_broadcasts() {
        let hub = WsHub::new();
        drop(connect(&hub, "gone", "u1"));
        let mut live = connect(&hub, "live", "u2");
        hub.join_board("gone", "board1");
        hub.join_board("live", "board1");

        hub.broadcast_to_board_all("board1", &events::list_deleted("l1"));
        assert_eq!(received_types(&mut live), ["LIST_DELETED"]);
    }
}
