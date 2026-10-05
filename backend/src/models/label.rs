use serde::Serialize;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Label {
    pub id: String,
    pub board_id: String,
    pub name: String,
    pub color: String,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct CardLabel {
    pub card_id: String,
    pub label_id: String,
}
