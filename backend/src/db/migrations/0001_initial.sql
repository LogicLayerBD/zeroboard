-- Initial schema. See SPEC.md "Database Schema" and .cursor/rules/03-database.mdc.
-- Primary keys are UUID strings, timestamps are Unix milliseconds,
-- booleans are INTEGER 0/1, positions are REAL for fractional indexing.

CREATE TABLE users (
  id           TEXT PRIMARY KEY,
  email        TEXT UNIQUE NOT NULL,
  name         TEXT NOT NULL,
  password     TEXT NOT NULL,
  avatar_color TEXT NOT NULL DEFAULT '#6366f1',
  created_at   INTEGER NOT NULL,
  updated_at   INTEGER NOT NULL
);

CREATE TABLE workspaces (
  id         TEXT PRIMARY KEY,
  name       TEXT NOT NULL,
  created_by TEXT NOT NULL REFERENCES users(id),
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE TABLE workspace_members (
  workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
  user_id      TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  role         TEXT NOT NULL CHECK(role IN ('admin','member','viewer')),
  joined_at    INTEGER NOT NULL,
  PRIMARY KEY (workspace_id, user_id)
);

-- Boards are archived, never hard-deleted, so children do not cascade from boards.
-- Deleting a workspace archives its boards first; workspace_id is then nulled
-- so board data survives the workspace.
CREATE TABLE boards (
  id           TEXT PRIMARY KEY,
  workspace_id TEXT REFERENCES workspaces(id) ON DELETE SET NULL,
  name         TEXT NOT NULL,
  archived     INTEGER NOT NULL DEFAULT 0,
  created_by   TEXT NOT NULL REFERENCES users(id),
  created_at   INTEGER NOT NULL,
  updated_at   INTEGER NOT NULL
);

CREATE TABLE lists (
  id         TEXT PRIMARY KEY,
  board_id   TEXT NOT NULL REFERENCES boards(id),
  name       TEXT NOT NULL,
  position   REAL NOT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE TABLE cards (
  id          TEXT PRIMARY KEY,
  list_id     TEXT NOT NULL REFERENCES lists(id) ON DELETE CASCADE,
  board_id    TEXT NOT NULL REFERENCES boards(id),
  title       TEXT NOT NULL,
  description TEXT,
  position    REAL NOT NULL,
  due_date    INTEGER,
  created_by  TEXT NOT NULL REFERENCES users(id),
  created_at  INTEGER NOT NULL,
  updated_at  INTEGER NOT NULL
);

CREATE TABLE card_assignees (
  card_id TEXT NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
  user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  PRIMARY KEY (card_id, user_id)
);

CREATE TABLE labels (
  id       TEXT PRIMARY KEY,
  board_id TEXT NOT NULL REFERENCES boards(id),
  name     TEXT NOT NULL,
  color    TEXT NOT NULL
);

CREATE TABLE card_labels (
  card_id  TEXT NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
  label_id TEXT NOT NULL REFERENCES labels(id) ON DELETE CASCADE,
  PRIMARY KEY (card_id, label_id)
);

CREATE TABLE attachments (
  id          TEXT PRIMARY KEY,
  card_id     TEXT NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
  filename    TEXT NOT NULL,
  stored_path TEXT NOT NULL,
  size_bytes  INTEGER NOT NULL,
  uploaded_by TEXT NOT NULL REFERENCES users(id),
  uploaded_at INTEGER NOT NULL
);

CREATE TABLE time_entries (
  id          TEXT PRIMARY KEY,
  card_id     TEXT NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
  user_id     TEXT NOT NULL REFERENCES users(id),
  minutes     INTEGER NOT NULL,
  description TEXT,
  logged_at   INTEGER NOT NULL
);

CREATE TABLE comments (
  id         TEXT PRIMARY KEY,
  card_id    TEXT NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
  user_id    TEXT NOT NULL REFERENCES users(id),
  body       TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

-- Activity history outlives its card: card_id is nulled instead of cascading.
CREATE TABLE activity_log (
  id         TEXT PRIMARY KEY,
  card_id    TEXT REFERENCES cards(id) ON DELETE SET NULL,
  board_id   TEXT REFERENCES boards(id),
  user_id    TEXT NOT NULL REFERENCES users(id),
  action     TEXT NOT NULL,
  payload    TEXT,
  created_at INTEGER NOT NULL
);

CREATE TABLE notifications (
  id         TEXT PRIMARY KEY,
  user_id    TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  type       TEXT NOT NULL,
  payload    TEXT NOT NULL,
  read       INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL
);

CREATE TABLE refresh_tokens (
  id         TEXT PRIMARY KEY,
  user_id    TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  token_hash TEXT NOT NULL,
  expires_at INTEGER NOT NULL,
  created_at INTEGER NOT NULL
);

-- Foreign-key and filter indexes. Composite (…, created_at) indexes back
-- cursor-based pagination. Leading PK columns of join tables are already indexed.
CREATE INDEX idx_workspaces_created_by ON workspaces(created_by);
CREATE INDEX idx_workspace_members_user_id ON workspace_members(user_id);
CREATE INDEX idx_boards_workspace_id ON boards(workspace_id);
CREATE INDEX idx_boards_created_by ON boards(created_by);
CREATE INDEX idx_lists_board_id ON lists(board_id, position);
CREATE INDEX idx_cards_list_id ON cards(list_id, position);
CREATE INDEX idx_cards_board_id ON cards(board_id);
CREATE INDEX idx_cards_created_by ON cards(created_by);
CREATE INDEX idx_card_assignees_user_id ON card_assignees(user_id);
CREATE INDEX idx_labels_board_id ON labels(board_id);
CREATE INDEX idx_card_labels_label_id ON card_labels(label_id);
CREATE INDEX idx_attachments_card_id ON attachments(card_id);
CREATE INDEX idx_attachments_uploaded_by ON attachments(uploaded_by);
CREATE INDEX idx_time_entries_card_id ON time_entries(card_id);
CREATE INDEX idx_time_entries_user_id ON time_entries(user_id);
CREATE INDEX idx_comments_card_id ON comments(card_id, created_at);
CREATE INDEX idx_comments_user_id ON comments(user_id);
CREATE INDEX idx_activity_log_card_id ON activity_log(card_id, created_at);
CREATE INDEX idx_activity_log_board_id ON activity_log(board_id, created_at);
CREATE INDEX idx_activity_log_user_id ON activity_log(user_id);
CREATE INDEX idx_notifications_user_id ON notifications(user_id, created_at);
CREATE INDEX idx_refresh_tokens_user_id ON refresh_tokens(user_id);
CREATE UNIQUE INDEX idx_refresh_tokens_token_hash ON refresh_tokens(token_hash);
