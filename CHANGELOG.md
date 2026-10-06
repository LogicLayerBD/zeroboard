# Changelog

All notable changes to ZeroBoard are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and versions follow
[Semantic Versioning](https://semver.org/).

## [1.0.0] - 2026-10-06

Real-time Kanban + List project management for small teams.
One binary. No Docker required. No Postgres. No Redis.

### Install

```bash
curl -LO https://github.com/LogicLayerBD/zeroboard/releases/download/v1.0.0/zeroboard-linux-x86_64
curl -LO https://github.com/LogicLayerBD/zeroboard/releases/download/v1.0.0/zeroboard-linux-x86_64.sha256
sha256sum -c zeroboard-linux-x86_64.sha256
mv zeroboard-linux-x86_64 zeroboard && chmod +x zeroboard

curl -L https://raw.githubusercontent.com/LogicLayerBD/zeroboard/v1.0.0/.env.example -o .env
sed -i "s/^JWT_SECRET=.*/JWT_SECRET=$(openssl rand -hex 32)/" .env
./zeroboard
```

Open http://localhost:3000. The first account becomes admin.

For access from other machines, serve it over HTTPS (see the reverse proxy section of the
README): sign-in cookies are `Secure`, so browsers drop them over plain HTTP.

### What's included

- Kanban and List views with real-time drag-and-drop sync
- Card details: Markdown description, assignees, due dates, labels, file attachments,
  time tracking, comments and activity log
- Workspaces with Admin / Member / Viewer roles
- In-app notifications and board presence
- Admin panel: server info, storage usage, user management (create users with a one-time
  password, reset passwords, change roles, deactivate / reactivate) and a public sign-up toggle
- Change your own password; show/hide on password fields
- SQLite in WAL mode, created and migrated automatically: no database setup

### Security

- Short-lived JWT access tokens plus rotating, hashed refresh tokens in an HttpOnly cookie
- Deactivating a user signs them out everywhere: sessions are revoked and new API or
  WebSocket requests are rejected
- Login and sign-up rate limits; strict Content-Security-Policy
- Tested: every protected route rejects missing, forged, expired and revoked tokens, and
  passwords, tokens and hashes never appear in logs

### Operations

- `/live` and `/ready` health checks
- `scripts/backup.sh` / `scripts/restore.sh` for the database plus attachments
- `make deploy` for one-command upgrades of from-source installs

### Performance (measured on a production VPS)

- Binary size: 7.2 MB
- Memory at idle: ~10 MB
- Startup: ~20 ms from launch to ready

### Requirements

- Linux x86_64 with glibc 2.35+ (Ubuntu 22.04+, Debian 12+); older systems can build from source
- 512 MB RAM minimum
- Disk space for attachments

### Docker

A Dockerfile and compose file are included for building your own image from a source
checkout; see the README.

### Documentation

See the [README](https://github.com/LogicLayerBD/zeroboard#readme) for reverse proxy, systemd,
backups, upgrades and environment variables.

[1.0.0]: https://github.com/LogicLayerBD/zeroboard/releases/tag/v1.0.0
