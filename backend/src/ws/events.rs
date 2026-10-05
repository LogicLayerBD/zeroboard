//! WebSocket event protocol (see SPEC.md "WebSocket Event Protocol"). Every
//! message is `{ "type": "EVENT_TYPE", "payload": { ... } }`.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::models::{Card, List, Notification};
use crate::services::workspaces::MemberDetails;

pub const CARD_CREATED: &str = "CARD_CREATED";
pub const CARD_UPDATED: &str = "CARD_UPDATED";
pub const CARD_DELETED: &str = "CARD_DELETED";
pub const CARD_MOVED: &str = "CARD_MOVED";
pub const LIST_CREATED: &str = "LIST_CREATED";
pub const LIST_UPDATED: &str = "LIST_UPDATED";
pub const LIST_DELETED: &str = "LIST_DELETED";
pub const LIST_REORDERED: &str = "LIST_REORDERED";
pub const MEMBER_JOINED: &str = "MEMBER_JOINED";
pub const MEMBER_LEFT: &str = "MEMBER_LEFT";
pub const PRESENCE_UPDATE: &str = "PRESENCE_UPDATE";
pub const NOTIFICATION: &str = "NOTIFICATION";
pub const PING: &str = "PING";

/// Server → client event.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WsEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    #[serde(skip_serializing_if = "Value::is_null")]
    pub payload: Value,
}

impl WsEvent {
    pub fn new(event_type: &str, payload: Value) -> Self {
        Self {
            event_type: event_type.to_string(),
            payload,
        }
    }
}

pub fn card_created(card: &Card) -> WsEvent {
    WsEvent::new(CARD_CREATED, json!({ "card": card }))
}

pub fn card_updated(card: &Card) -> WsEvent {
    WsEvent::new(CARD_UPDATED, json!({ "card": card }))
}

pub fn card_deleted(card_id: &str, list_id: &str) -> WsEvent {
    WsEvent::new(
        CARD_DELETED,
        json!({ "cardId": card_id, "listId": list_id }),
    )
}

pub fn card_moved(card_id: &str, from_list_id: &str, to_list_id: &str, position: f64) -> WsEvent {
    WsEvent::new(
        CARD_MOVED,
        json!({
            "cardId": card_id,
            "fromListId": from_list_id,
            "toListId": to_list_id,
            "position": position,
        }),
    )
}

pub fn list_created(list: &List) -> WsEvent {
    WsEvent::new(LIST_CREATED, json!({ "list": list }))
}

pub fn list_updated(list: &List) -> WsEvent {
    WsEvent::new(LIST_UPDATED, json!({ "list": list }))
}

pub fn list_deleted(list_id: &str) -> WsEvent {
    WsEvent::new(LIST_DELETED, json!({ "listId": list_id }))
}

pub fn list_reordered(list_id: &str, position: f64) -> WsEvent {
    WsEvent::new(
        LIST_REORDERED,
        json!({ "listId": list_id, "position": position }),
    )
}

pub fn member_joined(user: &MemberDetails, board_id: &str) -> WsEvent {
    WsEvent::new(MEMBER_JOINED, json!({ "user": user, "boardId": board_id }))
}

pub fn member_left(user_id: &str, board_id: &str) -> WsEvent {
    WsEvent::new(
        MEMBER_LEFT,
        json!({ "userId": user_id, "boardId": board_id }),
    )
}

pub fn presence_update(board_id: &str, active_users: &[String]) -> WsEvent {
    WsEvent::new(
        PRESENCE_UPDATE,
        json!({ "boardId": board_id, "activeUsers": active_users }),
    )
}

pub fn notification(notification: &Notification) -> WsEvent {
    WsEvent::new(NOTIFICATION, json!({ "notification": notification }))
}

pub fn ping() -> WsEvent {
    WsEvent::new(PING, Value::Null)
}

/// Client → server event. Unknown types fail to parse and are ignored.
#[derive(Debug, PartialEq, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ClientEvent {
    JoinBoard(BoardRef),
    LeaveBoard(BoardRef),
    Pong,
}

#[derive(Debug, PartialEq, Deserialize)]
pub struct BoardRef {
    #[serde(rename = "boardId", alias = "board_id")]
    pub board_id: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_serialize_with_type_and_camel_case_payload() {
        let event = card_moved("c1", "l1", "l2", 1500.0);
        assert_eq!(
            serde_json::to_value(&event).unwrap(),
            json!({
                "type": "CARD_MOVED",
                "payload": { "cardId": "c1", "fromListId": "l1", "toListId": "l2", "position": 1500.0 }
            })
        );
        assert_eq!(
            serde_json::to_value(presence_update("b1", &["u1".into(), "u2".into()])).unwrap(),
            json!({ "type": "PRESENCE_UPDATE", "payload": { "boardId": "b1", "activeUsers": ["u1", "u2"] } })
        );
    }

    #[test]
    fn ping_has_no_payload() {
        assert_eq!(
            serde_json::to_string(&ping()).unwrap(),
            r#"{"type":"PING"}"#
        );
    }

    #[test]
    fn parses_client_events() {
        let join: ClientEvent =
            serde_json::from_str(r#"{"type":"JOIN_BOARD","payload":{"boardId":"b1"}}"#).unwrap();
        assert_eq!(
            join,
            ClientEvent::JoinBoard(BoardRef {
                board_id: "b1".into()
            })
        );
        let leave: ClientEvent =
            serde_json::from_str(r#"{"type":"LEAVE_BOARD","payload":{"board_id":"b1"}}"#).unwrap();
        assert_eq!(
            leave,
            ClientEvent::LeaveBoard(BoardRef {
                board_id: "b1".into()
            })
        );
        let pong: ClientEvent = serde_json::from_str(r#"{"type":"PONG"}"#).unwrap();
        assert_eq!(pong, ClientEvent::Pong);
    }

    #[test]
    fn rejects_unknown_or_malformed_client_events() {
        assert!(
            serde_json::from_str::<ClientEvent>(r#"{"type":"CARD_CREATED","payload":{}}"#).is_err()
        );
        assert!(serde_json::from_str::<ClientEvent>(r#"{"type":"JOIN_BOARD"}"#).is_err());
        assert!(serde_json::from_str::<ClientEvent>("not json").is_err());
    }
}
