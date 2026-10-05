use serde::Serialize;
use serde_json::json;

use super::positions::{reposition, slot_after, Sibling, POSITION_STEP};
use super::{activity, is_foreign_key_violation, is_unique_violation, notifications, now_ms};
use crate::db;
use crate::errors::AppError;
use crate::models::{Attachment, Card, CardAssignee, CardLabel, Comment, Label, TimeEntry};
use crate::ws::events;
use crate::AppState;

const TARGET_LIST_MESSAGE: &str = "list_id must be a list on this board";
const AFTER_ID_MESSAGE: &str = "after_id must be another card in the target list";
const NOT_WORKSPACE_MEMBER_MESSAGE: &str = "assignee must be a member of this workspace";
const ALREADY_ASSIGNED_MESSAGE: &str = "user is already assigned to this card";
const LABEL_NOT_ON_BOARD_MESSAGE: &str = "label must belong to this board";
const ALREADY_LABELED_MESSAGE: &str = "label is already on this card";

/// Public profile of an assignee.
#[derive(Debug, Serialize)]
pub struct AssigneeDetails {
    pub user_id: String,
    pub name: String,
    pub email: String,
    pub avatar_color: String,
}

#[derive(Debug, Serialize)]
pub struct CardDetails {
    #[serde(flatten)]
    pub card: Card,
    pub assignees: Vec<AssigneeDetails>,
    pub labels: Vec<Label>,
    pub attachments: Vec<Attachment>,
    pub time_entries: Vec<TimeEntry>,
    pub comments: Vec<Comment>,
}

pub struct NewCard {
    pub title: String,
    pub description: Option<String>,
    pub due_date: Option<i64>,
}

/// Partial update: `None` leaves a field unchanged, `Some(None)` clears a nullable one.
pub struct CardChanges {
    pub title: Option<String>,
    pub description: Option<Option<String>>,
    pub due_date: Option<Option<i64>>,
}

pub async fn list_for_list(state: &AppState, list_id: &str) -> Result<Vec<Card>, AppError> {
    let cards = sqlx::query_as!(
        Card,
        r#"SELECT id AS "id!", list_id, board_id, title, description, position, due_date,
                  created_by, created_at, updated_at
           FROM cards
           WHERE list_id = $1
           ORDER BY position, id"#,
        list_id
    )
    .fetch_all(&state.read_db)
    .await?;
    Ok(cards)
}

/// Appends the card after the list's last card.
pub async fn create(
    state: &AppState,
    list_id: &str,
    board_id: &str,
    user_id: &str,
    new_card: NewCard,
) -> Result<Card, AppError> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = now_ms();
    let inserted = sqlx::query_as!(
        Card,
        r#"INSERT INTO cards (id, list_id, board_id, title, description, position, due_date,
                              created_by, created_at, updated_at)
           VALUES ($1, $2, $3, $4, $5,
                   COALESCE((SELECT MAX(position) FROM cards WHERE list_id = $2), 0.0) + $6,
                   $7, $8, $9, $9)
           RETURNING id AS "id!", list_id AS "list_id!", board_id AS "board_id!",
                     title AS "title!", description, position AS "position!", due_date,
                     created_by AS "created_by!", created_at AS "created_at!",
                     updated_at AS "updated_at!""#,
        id,
        list_id,
        board_id,
        new_card.title,
        new_card.description,
        POSITION_STEP,
        new_card.due_date,
        user_id,
        now
    )
    .fetch_all(&state.db)
    .await
    .and_then(db::single_row);

    let card = match inserted {
        Ok(card) => card,
        // The list was deleted after the access check.
        Err(err) if is_foreign_key_violation(&err) => return Err(AppError::NotFound),
        Err(err) => return Err(err.into()),
    };
    tracing::info!(card_id = %card.id, %list_id, %user_id, "card created");
    activity::record(
        state,
        &card.id,
        board_id,
        user_id,
        activity::CREATED_CARD,
        json!({ "list_id": list_id, "title": card.title }),
    )
    .await;
    state
        .ws_hub
        .broadcast_to_board_all(board_id, &events::card_created(&card));
    Ok(card)
}

pub async fn details(state: &AppState, card: Card) -> Result<CardDetails, AppError> {
    let assignees = sqlx::query_as!(
        AssigneeDetails,
        r#"SELECT u.id AS "user_id!", u.name, u.email, u.avatar_color
           FROM card_assignees a
           JOIN users u ON u.id = a.user_id
           WHERE a.card_id = $1
           ORDER BY u.name, u.id"#,
        card.id
    )
    .fetch_all(&state.read_db)
    .await?;

    let labels = sqlx::query_as!(
        Label,
        r#"SELECT l.id AS "id!", l.board_id, l.name, l.color
           FROM card_labels cl
           JOIN labels l ON l.id = cl.label_id
           WHERE cl.card_id = $1
           ORDER BY l.name, l.id"#,
        card.id
    )
    .fetch_all(&state.read_db)
    .await?;

    let attachments = sqlx::query_as!(
        Attachment,
        r#"SELECT id AS "id!", card_id, filename, stored_path, size_bytes, uploaded_by,
                  uploaded_at
           FROM attachments
           WHERE card_id = $1
           ORDER BY uploaded_at, id"#,
        card.id
    )
    .fetch_all(&state.read_db)
    .await?;

    let time_entries = sqlx::query_as!(
        TimeEntry,
        r#"SELECT id AS "id!", card_id, user_id, minutes, description, logged_at
           FROM time_entries
           WHERE card_id = $1
           ORDER BY logged_at, id"#,
        card.id
    )
    .fetch_all(&state.read_db)
    .await?;

    let comments = sqlx::query_as!(
        Comment,
        r#"SELECT id AS "id!", card_id, user_id, body, created_at, updated_at
           FROM comments
           WHERE card_id = $1
           ORDER BY created_at, id"#,
        card.id
    )
    .fetch_all(&state.read_db)
    .await?;

    Ok(CardDetails {
        card,
        assignees,
        labels,
        attachments,
        time_entries,
        comments,
    })
}

/// Notifies assignees (except `actor_id`) when the updated card is due within 24 hours.
pub async fn update(
    state: &AppState,
    actor_id: &str,
    card_id: &str,
    changes: CardChanges,
) -> Result<Card, AppError> {
    let set_description = changes.description.is_some();
    let description = changes.description.flatten();
    let set_due_date = changes.due_date.is_some();
    let due_date = changes.due_date.flatten();
    let now = now_ms();
    let card = sqlx::query_as!(
        Card,
        r#"UPDATE cards
           SET title = COALESCE($1, title),
               description = CASE WHEN $2 THEN $3 ELSE description END,
               due_date = CASE WHEN $4 THEN $5 ELSE due_date END,
               updated_at = $6
           WHERE id = $7
           RETURNING id AS "id!", list_id AS "list_id!", board_id AS "board_id!",
                     title AS "title!", description, position AS "position!", due_date,
                     created_by AS "created_by!", created_at AS "created_at!",
                     updated_at AS "updated_at!""#,
        changes.title,
        set_description,
        description,
        set_due_date,
        due_date,
        now,
        card_id
    )
    .fetch_all(&state.db)
    .await
    .map(db::first_row)?
    .ok_or(AppError::NotFound)?;

    tracing::info!(%card_id, %actor_id, "card updated");
    state
        .ws_hub
        .broadcast_to_board_all(&card.board_id, &events::card_updated(&card));
    notifications::notify_if_due_soon(state, actor_id, &card).await;
    Ok(card)
}

/// Deletes the card and its dependent rows in one transaction, then removes its
/// attachment files. File cleanup failures are logged and do not fail the request.
pub async fn delete(state: &AppState, card_id: &str) -> Result<(), AppError> {
    let mut tx = state.db.begin().await?;
    sqlx::query!("DELETE FROM card_assignees WHERE card_id = $1", card_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query!("DELETE FROM card_labels WHERE card_id = $1", card_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query!("DELETE FROM time_entries WHERE card_id = $1", card_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query!("DELETE FROM comments WHERE card_id = $1", card_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query!("DELETE FROM attachments WHERE card_id = $1", card_id)
        .execute(&mut *tx)
        .await?;
    let deleted = sqlx::query!(
        r#"DELETE FROM cards WHERE id = $1
           RETURNING list_id AS "list_id!", board_id AS "board_id!""#,
        card_id
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(AppError::NotFound)?;
    tx.commit().await?;

    tracing::info!(%card_id, "card deleted");
    state.ws_hub.broadcast_to_board_all(
        &deleted.board_id,
        &events::card_deleted(card_id, &deleted.list_id),
    );
    remove_attachment_files(state, card_id).await;
    Ok(())
}

/// Removes `{ATTACHMENTS_DIR}/{card_id}/`. `card_id` must be a validated UUID.
pub async fn remove_attachment_files(state: &AppState, card_id: &str) {
    let dir = state.config.attachments_dir.join(card_id);
    match tokio::fs::remove_dir_all(&dir).await {
        Ok(()) => tracing::info!(%card_id, "card attachment files removed"),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => tracing::error!(
            error = ?err,
            %card_id,
            path = %dir.display(),
            "failed to remove card attachment files"
        ),
    }
}

/// Moves the card into `to_list_id` (same board) directly after `after_id`,
/// or first when `None`.
pub async fn move_card(
    state: &AppState,
    user_id: &str,
    card: &Card,
    to_list_id: &str,
    after_id: Option<&str>,
) -> Result<Card, AppError> {
    let mut tx = state.db.begin().await?;
    let target_board = sqlx::query!("SELECT board_id FROM lists WHERE id = $1", to_list_id)
        .fetch_optional(&mut *tx)
        .await?
        .map(|row| row.board_id);
    if target_board.as_deref() != Some(card.board_id.as_str()) {
        return Err(AppError::BadRequest(TARGET_LIST_MESSAGE.to_string()));
    }

    let siblings = sqlx::query_as!(
        Sibling,
        r#"SELECT id AS "id!", position
           FROM cards
           WHERE list_id = $1 AND id != $2
           ORDER BY position, id"#,
        to_list_id,
        card.id
    )
    .fetch_all(&mut *tx)
    .await?;
    let slot = slot_after(&siblings, after_id)
        .ok_or_else(|| AppError::BadRequest(AFTER_ID_MESSAGE.to_string()))?;
    let plan = reposition(&siblings, slot);

    for (id, position) in &plan.rebalanced {
        sqlx::query!("UPDATE cards SET position = $1 WHERE id = $2", position, id)
            .execute(&mut *tx)
            .await?;
    }
    let now = now_ms();
    let moved = sqlx::query_as!(
        Card,
        r#"UPDATE cards SET list_id = $1, position = $2, updated_at = $3
           WHERE id = $4
           RETURNING id AS "id!", list_id AS "list_id!", board_id AS "board_id!",
                     title AS "title!", description, position AS "position!", due_date,
                     created_by AS "created_by!", created_at AS "created_at!",
                     updated_at AS "updated_at!""#,
        to_list_id,
        plan.position,
        now,
        card.id
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(AppError::NotFound)?;
    tx.commit().await?;

    tracing::info!(
        card_id = %moved.id,
        from_list_id = %card.list_id,
        to_list_id = %moved.list_id,
        position = moved.position,
        rebalanced = !plan.rebalanced.is_empty(),
        "card moved"
    );
    state.ws_hub.broadcast_to_board_all(
        &moved.board_id,
        &events::card_moved(&moved.id, &card.list_id, &moved.list_id, moved.position),
    );
    activity::record(
        state,
        &moved.id,
        &moved.board_id,
        user_id,
        activity::MOVED_CARD,
        json!({
            "from_list_id": card.list_id,
            "to_list_id": moved.list_id,
            "position": moved.position,
        }),
    )
    .await;
    Ok(moved)
}

pub async fn add_assignee(
    state: &AppState,
    actor_id: &str,
    card: &Card,
    assignee_id: &str,
) -> Result<CardAssignee, AppError> {
    let member = sqlx::query!(
        r#"SELECT m.user_id AS "user_id!"
           FROM workspace_members m
           JOIN boards b ON b.workspace_id = m.workspace_id
           WHERE b.id = $1 AND m.user_id = $2"#,
        card.board_id,
        assignee_id
    )
    .fetch_optional(&state.read_db)
    .await?;
    if member.is_none() {
        return Err(AppError::BadRequest(
            NOT_WORKSPACE_MEMBER_MESSAGE.to_string(),
        ));
    }

    let inserted = sqlx::query!(
        "INSERT INTO card_assignees (card_id, user_id) VALUES ($1, $2)",
        card.id,
        assignee_id
    )
    .execute(&state.db)
    .await;
    match inserted {
        Ok(_) => {}
        Err(err) if is_unique_violation(&err) => {
            return Err(AppError::BadRequest(ALREADY_ASSIGNED_MESSAGE.to_string()));
        }
        // The card or user was deleted after the checks above.
        Err(err) if is_foreign_key_violation(&err) => return Err(AppError::NotFound),
        Err(err) => return Err(err.into()),
    }

    tracing::info!(card_id = %card.id, %assignee_id, %actor_id, "card assignee added");
    activity::record(
        state,
        &card.id,
        &card.board_id,
        actor_id,
        activity::ADDED_ASSIGNEE,
        json!({ "user_id": assignee_id }),
    )
    .await;
    notifications::notify_assigned(state, actor_id, card, assignee_id).await;
    Ok(CardAssignee {
        card_id: card.id.clone(),
        user_id: assignee_id.to_string(),
    })
}

pub async fn remove_assignee(
    state: &AppState,
    actor_id: &str,
    card: &Card,
    assignee_id: &str,
) -> Result<(), AppError> {
    let deleted = sqlx::query!(
        "DELETE FROM card_assignees WHERE card_id = $1 AND user_id = $2",
        card.id,
        assignee_id
    )
    .execute(&state.db)
    .await?;
    if deleted.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }

    tracing::info!(card_id = %card.id, %assignee_id, %actor_id, "card assignee removed");
    activity::record(
        state,
        &card.id,
        &card.board_id,
        actor_id,
        activity::REMOVED_ASSIGNEE,
        json!({ "user_id": assignee_id }),
    )
    .await;
    Ok(())
}

pub async fn add_label(
    state: &AppState,
    actor_id: &str,
    card: &Card,
    label_id: &str,
) -> Result<CardLabel, AppError> {
    let label = sqlx::query!(
        r#"SELECT id AS "id!" FROM labels WHERE id = $1 AND board_id = $2"#,
        label_id,
        card.board_id
    )
    .fetch_optional(&state.read_db)
    .await?;
    if label.is_none() {
        return Err(AppError::BadRequest(LABEL_NOT_ON_BOARD_MESSAGE.to_string()));
    }

    let inserted = sqlx::query!(
        "INSERT INTO card_labels (card_id, label_id) VALUES ($1, $2)",
        card.id,
        label_id
    )
    .execute(&state.db)
    .await;
    match inserted {
        Ok(_) => {}
        Err(err) if is_unique_violation(&err) => {
            return Err(AppError::BadRequest(ALREADY_LABELED_MESSAGE.to_string()));
        }
        // The card or label was deleted after the checks above.
        Err(err) if is_foreign_key_violation(&err) => return Err(AppError::NotFound),
        Err(err) => return Err(err.into()),
    }

    tracing::info!(card_id = %card.id, %label_id, %actor_id, "card label added");
    activity::record(
        state,
        &card.id,
        &card.board_id,
        actor_id,
        activity::ADDED_LABEL,
        json!({ "label_id": label_id }),
    )
    .await;
    Ok(CardLabel {
        card_id: card.id.clone(),
        label_id: label_id.to_string(),
    })
}

pub async fn remove_label(
    state: &AppState,
    actor_id: &str,
    card: &Card,
    label_id: &str,
) -> Result<(), AppError> {
    let deleted = sqlx::query!(
        "DELETE FROM card_labels WHERE card_id = $1 AND label_id = $2",
        card.id,
        label_id
    )
    .execute(&state.db)
    .await?;
    if deleted.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }

    tracing::info!(card_id = %card.id, %label_id, %actor_id, "card label removed");
    activity::record(
        state,
        &card.id,
        &card.board_id,
        actor_id,
        activity::REMOVED_LABEL,
        json!({ "label_id": label_id }),
    )
    .await;
    Ok(())
}
