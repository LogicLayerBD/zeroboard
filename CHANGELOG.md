# Changelog

All notable changes to ZeroBoard are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and versions follow
[Semantic Versioning](https://semver.org/).

## [1.0.0] - 2026-10-06

First stable release: a self-hosted Kanban + List board for small teams, shipped as a
single Linux binary with an embedded frontend and SQLite database.

### Features

- **Boards** — workspaces with Admin / Member / Viewer roles, boards, lists and cards;
  drag-and-drop for cards and lists; quick-add.
- **List view** — the same cards as a sortable table.
- **Card details** — Markdown description, assignees, due dates, colored labels, file
  attachments on local disk, time tracking, comments and an activity log.
- **Real-time sync** — changes and board presence appear instantly over WebSockets.
- **In-app notifications** — assignments, upcoming due dates and comments on your cards.
- **Admin panel** — server info, attachment storage per workspace, and user management:
  search, create users with a one-time temporary password, change instance roles,
  reset passwords, deactivate / reactivate, and a public sign-up on/off toggle.
- **Invite autocomplete** for instance admins.
- **Accounts** — change your own password (signs out other sessions); show/hide toggle on
  password fields.

### Security

- Short-lived JWT access tokens plus rotating, hashed refresh tokens in an
  `HttpOnly; Secure; SameSite=Strict` cookie; bcrypt password hashing.
- Deactivation takes effect immediately for existing tokens and WebSocket connections.
- Login and registration rate limits; strict Content-Security-Policy.
- Automated tests check that every protected route rejects missing, forged, expired and
  revoked tokens, and that passwords, tokens and hashes never appear in logs.

### Operations

- One binary, no runtime dependencies; migrations run automatically on startup.
- `/live` and `/ready` health checks.
- `scripts/backup.sh` / `scripts/restore.sh` for database + attachments, and
  `make deploy` for one-command upgrades of from-source installs.

[1.0.0]: https://github.com/LogicLayerBD/zeroboard/releases/tag/v1.0.0
