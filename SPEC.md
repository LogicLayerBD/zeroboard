# ZeroBoard — Project Specification v1.0

## Elevator Pitch
A real-time Kanban + List project management tool for small teams (2–5 people).
Single Rust binary, embedded SQLite, embedded Svelte frontend.
No Docker. No Postgres. No Redis. Drop it on a $5 VPS and run it.

---

## Tech Stack

| Layer | Choice | Reason |
|---|---|---|
| Backend | Rust + Axum + Tokio | Performance, single binary |
| Frontend | Svelte 5 + Vite | Compiles to tiny static files, embedded in binary |
| Real-time | WebSockets (tokio-tungstenite) | Card sync, cursor presence |
| Database | SQLite (sqlx + WAL mode) | Zero-infra, file-based |
| Embedding | rust-embed | Bakes frontend into binary |
| Auth | JWT (jsonwebtoken crate) | Stateless, simple |
| File Storage | Local disk (configurable path) | No S3 dependency for v1 |

---

## V1 Feature List (Locked Scope)

### Authentication
- [ ] Register (first user becomes admin)
- [ ] Login / Logout
- [ ] JWT access token (short-lived) + refresh token (long-lived, stored in httpOnly cookie)
- [ ] Basic profile: display name, avatar color

### Workspaces & Boards
- [ ] Create / rename / delete workspace
- [ ] Invite members to workspace by email
- [ ] Create / rename / archive boards inside a workspace
- [ ] Board-level permissions: Admin, Member, Viewer

### Kanban View
- [ ] Create / rename / delete lists (columns)
- [ ] Create / edit / delete cards
- [ ] Drag-and-drop cards between lists (real-time synced to all open clients)
- [ ] Drag-and-drop reorder lists
- [ ] Card quick-add (click + at bottom of list, type, Enter)

### List View
- [ ] Same cards displayed as sortable table rows
- [ ] Sort by: due date, assignee, label, creation date
- [ ] Inline edit: title, due date, assignee

### Card Detail (Modal)
- [ ] Title (editable inline)
- [ ] Description (Markdown, rendered on view)
- [ ] Assignees (multi-select from workspace members)
- [ ] Due date + time picker
- [ ] Labels (create custom labels with color per board)
- [ ] File attachments (upload, download, delete) — stored on local disk
- [ ] Time tracking: log time entries (description + minutes), show total
- [ ] Activity log (auto-generated: "John moved this card", "Sarah added attachment")
- [ ] Comments (text only for v1)

### Real-Time Sync
- [ ] Card moved → all clients update instantly
- [ ] Card created/deleted → reflected immediately
- [ ] Card title edited → live update
- [ ] Presence indicators: show which members have a board open

### Notifications (In-App Only for v1)
- [ ] Assigned to a card
- [ ] Card due date within 24 hours
- [ ] Someone comments on a card you're assigned to
- [ ] Notification bell in navbar with unread count

### Admin Panel
- [ ] Manage workspace members (invite, remove, change role)
- [ ] View storage usage (attachment sizes)
- [ ] Server info (version, uptime, DB size)

---

## Out of Scope for V1 (Do Not Build)
- Email notifications (SMTP)
- Calendar view
- Gantt / Timeline view
- Public boards
- OAuth (Google, GitHub login)
- Mobile app
- Recurring cards
- Card dependencies / blocking relationships
- Integrations (Slack, webhooks)
- Multiple file storage backends (S3, etc.)

---

## Database Schema

### users
```sql
CREATE TABLE users (
  id          TEXT PRIMARY KEY,  -- UUID
  email       TEXT UNIQUE NOT NULL,
  name        TEXT NOT NULL,
  password    TEXT NOT NULL,     -- bcrypt hash
  avatar_color TEXT NOT NULL DEFAULT '#6366f1',
  created_at  INTEGER NOT NULL,
  updated_at  INTEGER NOT NULL
);
```

### workspaces
```sql
CREATE TABLE workspaces (
  id         TEXT PRIMARY KEY,
  name       TEXT NOT NULL,
  created_by TEXT NOT NULL REFERENCES users(id),
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
```

### workspace_members
```sql
CREATE TABLE workspace_members (
  workspace_id TEXT NOT NULL REFERENCES workspaces(id),
  user_id      TEXT NOT NULL REFERENCES users(id),
  role         TEXT NOT NULL CHECK(role IN ('admin','member','viewer')),
  joined_at    INTEGER NOT NULL,
  PRIMARY KEY (workspace_id, user_id)
);
```

### boards
```sql
CREATE TABLE boards (
  id           TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL REFERENCES workspaces(id),
  name         TEXT NOT NULL,
  archived     INTEGER NOT NULL DEFAULT 0,
  created_by   TEXT NOT NULL REFERENCES users(id),
  created_at   INTEGER NOT NULL,
  updated_at   INTEGER NOT NULL
);
```

### lists
```sql
CREATE TABLE lists (
  id         TEXT PRIMARY KEY,
  board_id   TEXT NOT NULL REFERENCES boards(id),
  name       TEXT NOT NULL,
  position   REAL NOT NULL,   -- fractional indexing for ordering
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
```

### cards
```sql
CREATE TABLE cards (
  id          TEXT PRIMARY KEY,
  list_id     TEXT NOT NULL REFERENCES lists(id),
  board_id    TEXT NOT NULL REFERENCES boards(id),
  title       TEXT NOT NULL,
  description TEXT,
  position    REAL NOT NULL,  -- fractional indexing
  due_date    INTEGER,
  created_by  TEXT NOT NULL REFERENCES users(id),
  created_at  INTEGER NOT NULL,
  updated_at  INTEGER NOT NULL
);
```

### card_assignees
```sql
CREATE TABLE card_assignees (
  card_id TEXT NOT NULL REFERENCES cards(id),
  user_id TEXT NOT NULL REFERENCES users(id),
  PRIMARY KEY (card_id, user_id)
);
```

### labels
```sql
CREATE TABLE labels (
  id       TEXT PRIMARY KEY,
  board_id TEXT NOT NULL REFERENCES boards(id),
  name     TEXT NOT NULL,
  color    TEXT NOT NULL
);
```

### card_labels
```sql
CREATE TABLE card_labels (
  card_id  TEXT NOT NULL REFERENCES cards(id),
  label_id TEXT NOT NULL REFERENCES labels(id),
  PRIMARY KEY (card_id, label_id)
);
```

### attachments
```sql
CREATE TABLE attachments (
  id          TEXT PRIMARY KEY,
  card_id     TEXT NOT NULL REFERENCES cards(id),
  filename    TEXT NOT NULL,
  stored_path TEXT NOT NULL,
  size_bytes  INTEGER NOT NULL,
  uploaded_by TEXT NOT NULL REFERENCES users(id),
  uploaded_at INTEGER NOT NULL
);
```

### time_entries
```sql
CREATE TABLE time_entries (
  id          TEXT PRIMARY KEY,
  card_id     TEXT NOT NULL REFERENCES cards(id),
  user_id     TEXT NOT NULL REFERENCES users(id),
  minutes     INTEGER NOT NULL,
  description TEXT,
  logged_at   INTEGER NOT NULL
);
```

### comments
```sql
CREATE TABLE comments (
  id         TEXT PRIMARY KEY,
  card_id    TEXT NOT NULL REFERENCES cards(id),
  user_id    TEXT NOT NULL REFERENCES users(id),
  body       TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
```

### activity_log
```sql
CREATE TABLE activity_log (
  id         TEXT PRIMARY KEY,
  card_id    TEXT REFERENCES cards(id),
  board_id   TEXT REFERENCES boards(id),
  user_id    TEXT NOT NULL REFERENCES users(id),
  action     TEXT NOT NULL,  -- e.g. "moved_card", "added_attachment"
  payload    TEXT,           -- JSON string with context
  created_at INTEGER NOT NULL
);
```

### notifications
```sql
CREATE TABLE notifications (
  id         TEXT PRIMARY KEY,
  user_id    TEXT NOT NULL REFERENCES users(id),
  type       TEXT NOT NULL,
  payload    TEXT NOT NULL,  -- JSON
  read       INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL
);
```

### refresh_tokens
```sql
CREATE TABLE refresh_tokens (
  id         TEXT PRIMARY KEY,
  user_id    TEXT NOT NULL REFERENCES users(id),
  token_hash TEXT NOT NULL,
  expires_at INTEGER NOT NULL,
  created_at INTEGER NOT NULL
);
```

---

## API Routes

### Auth
```
POST   /api/auth/register
POST   /api/auth/login
POST   /api/auth/logout
POST   /api/auth/refresh
GET    /api/auth/me
```

### Workspaces
```
GET    /api/workspaces
POST   /api/workspaces
PATCH  /api/workspaces/:id
DELETE /api/workspaces/:id
GET    /api/workspaces/:id/members
POST   /api/workspaces/:id/members/invite
DELETE /api/workspaces/:id/members/:userId
PATCH  /api/workspaces/:id/members/:userId/role
```

### Boards
```
GET    /api/workspaces/:workspaceId/boards
POST   /api/workspaces/:workspaceId/boards
GET    /api/boards/:id
PATCH  /api/boards/:id
DELETE /api/boards/:id
```

### Lists
```
GET    /api/boards/:boardId/lists
POST   /api/boards/:boardId/lists
PATCH  /api/lists/:id
DELETE /api/lists/:id
PATCH  /api/lists/:id/position
```

### Cards
```
GET    /api/lists/:listId/cards
POST   /api/lists/:listId/cards
GET    /api/cards/:id
PATCH  /api/cards/:id
DELETE /api/cards/:id
PATCH  /api/cards/:id/move       -- change list + position
POST   /api/cards/:id/assignees
DELETE /api/cards/:id/assignees/:userId
POST   /api/cards/:id/labels
DELETE /api/cards/:id/labels/:labelId
```

### Attachments
```
POST   /api/cards/:id/attachments
GET    /api/attachments/:id/download
DELETE /api/attachments/:id
```

### Time Entries
```
GET    /api/cards/:id/time-entries
POST   /api/cards/:id/time-entries
DELETE /api/time-entries/:id
```

### Comments
```
GET    /api/cards/:id/comments
POST   /api/cards/:id/comments
PATCH  /api/comments/:id
DELETE /api/comments/:id
```

### Labels
```
GET    /api/boards/:boardId/labels
POST   /api/boards/:boardId/labels
PATCH  /api/labels/:id
DELETE /api/labels/:id
```

### Notifications
```
GET    /api/notifications
PATCH  /api/notifications/:id/read
POST   /api/notifications/read-all
```

### Admin
```
GET    /api/admin/info
GET    /api/admin/storage
```

### WebSocket
```
WS     /ws?token=<jwt>
```

---

## WebSocket Event Protocol

All events are JSON. Every event has:
```json
{ "type": "EVENT_TYPE", "payload": { ... } }
```

### Server → Client Events
```
CARD_CREATED       { card }
CARD_UPDATED       { card }
CARD_DELETED       { cardId, listId }
CARD_MOVED         { cardId, fromListId, toListId, position }
LIST_CREATED       { list }
LIST_UPDATED       { list }
LIST_DELETED       { listId }
LIST_REORDERED     { listId, position }
MEMBER_JOINED      { user, boardId }
MEMBER_LEFT        { userId, boardId }
PRESENCE_UPDATE    { boardId, activeUsers: [userId] }
NOTIFICATION       { notification }
```

### Client → Server Events
```
JOIN_BOARD         { boardId }
LEAVE_BOARD        { boardId }
```

---

## File Structure

```
zeroboard/
├── backend/
│   ├── src/
│   │   ├── main.rs
│   │   ├── config.rs
│   │   ├── db/
│   │   │   ├── mod.rs
│   │   │   └── migrations/
│   │   ├── models/
│   │   ├── handlers/
│   │   │   ├── auth.rs
│   │   │   ├── workspaces.rs
│   │   │   ├── boards.rs
│   │   │   ├── lists.rs
│   │   │   ├── cards.rs
│   │   │   ├── attachments.rs
│   │   │   ├── time_entries.rs
│   │   │   ├── comments.rs
│   │   │   ├── notifications.rs
│   │   │   └── admin.rs
│   │   ├── ws/
│   │   │   ├── mod.rs
│   │   │   └── events.rs
│   │   ├── auth/
│   │   │   └── jwt.rs
│   │   └── errors.rs
│   ├── Cargo.toml
│   └── build.rs          ← triggers frontend build, copies to assets/
├── frontend/
│   ├── src/
│   │   ├── lib/
│   │   │   ├── api.ts
│   │   │   ├── ws.ts
│   │   │   ├── stores/
│   │   │   └── components/
│   │   ├── routes/
│   │   │   ├── +layout.svelte
│   │   │   ├── login/
│   │   │   ├── register/
│   │   │   └── [workspace]/
│   │   │       └── [board]/
│   │   └── app.html
│   ├── package.json
│   └── vite.config.ts
├── .cursor/
│   └── rules/
│       ├── 00-project-overview.mdc
│       ├── 01-backend-rust.mdc
│       ├── 02-frontend-svelte.mdc
│       ├── 03-database.mdc
│       ├── 04-websockets.mdc
│       └── 05-security.mdc
├── SPEC.md               ← this file
├── README.md
├── .env.example
└── Makefile
```

---

## Environment Variables (.env.example)

```env
# Server
PORT=3000
HOST=0.0.0.0

# Security
JWT_SECRET=change-this-to-a-long-random-string
JWT_EXPIRY_MINUTES=15
REFRESH_TOKEN_EXPIRY_DAYS=30

# Database
DATABASE_URL=sqlite://data/zeroboard.db

# Storage
ATTACHMENTS_DIR=data/attachments
MAX_ATTACHMENT_SIZE_MB=25

# App
APP_NAME=ZeroBoard
FIRST_USER_IS_ADMIN=true
```

---

## Definition of Done — V1

The app is considered v1-complete when:
1. A fresh binary dropped on a Ubuntu VPS starts with no other dependencies
2. First user registers and becomes admin
3. Can invite 2–4 teammates by email (they register via invite link)
4. Can create a board, add lists, add cards, drag cards between lists — all synced live to other open browsers
5. Can open a card and: add assignees, set due date, add labels, attach a file, log time, leave a comment
6. List view works and is sortable
7. Notifications appear in the bell when assigned to a card
8. Binary RAM usage stays under 80MB with 5 concurrent users
9. All API routes return proper error messages (not 500s) for bad input
10. README has working install instructions

---

## Performance Targets

- Binary size: < 30MB
- Startup time: < 500ms
- RAM at idle (5 users): < 80MB
- WebSocket message latency: < 50ms on local network
- SQLite WAL mode enabled from first migration
