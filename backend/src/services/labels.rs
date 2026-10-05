use super::is_foreign_key_violation;
use crate::db;
use crate::errors::AppError;
use crate::models::Label;
use crate::AppState;

pub async fn list_for_board(state: &AppState, board_id: &str) -> Result<Vec<Label>, AppError> {
    let labels = sqlx::query_as!(
        Label,
        r#"SELECT id AS "id!", board_id, name, color
           FROM labels
           WHERE board_id = $1
           ORDER BY name, id"#,
        board_id
    )
    .fetch_all(&state.read_db)
    .await?;
    Ok(labels)
}

pub async fn create(
    state: &AppState,
    board_id: &str,
    name: &str,
    color: &str,
) -> Result<Label, AppError> {
    let id = uuid::Uuid::new_v4().to_string();
    let inserted = sqlx::query_as!(
        Label,
        r#"INSERT INTO labels (id, board_id, name, color)
           VALUES ($1, $2, $3, $4)
           RETURNING id AS "id!", board_id AS "board_id!", name AS "name!", color AS "color!""#,
        id,
        board_id,
        name,
        color
    )
    .fetch_all(&state.db)
    .await
    .and_then(db::single_row);

    let label = match inserted {
        Ok(label) => label,
        Err(err) if is_foreign_key_violation(&err) => return Err(AppError::NotFound),
        Err(err) => return Err(err.into()),
    };
    tracing::info!(label_id = %label.id, %board_id, "label created");
    Ok(label)
}

/// `None` leaves a field unchanged.
pub async fn update(
    state: &AppState,
    label_id: &str,
    name: Option<&str>,
    color: Option<&str>,
) -> Result<Label, AppError> {
    let label = sqlx::query_as!(
        Label,
        r#"UPDATE labels SET name = COALESCE($1, name), color = COALESCE($2, color)
           WHERE id = $3
           RETURNING id AS "id!", board_id AS "board_id!", name AS "name!", color AS "color!""#,
        name,
        color,
        label_id
    )
    .fetch_all(&state.db)
    .await
    .map(db::first_row)?
    .ok_or(AppError::NotFound)?;

    tracing::info!(%label_id, "label updated");
    Ok(label)
}

/// `card_labels` rows cascade, removing the label from every card.
pub async fn delete(state: &AppState, label_id: &str) -> Result<(), AppError> {
    let deleted = sqlx::query!("DELETE FROM labels WHERE id = $1", label_id)
        .execute(&state.db)
        .await?;
    if deleted.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    tracing::info!(%label_id, "label deleted");
    Ok(())
}
