# ZeroBoard

> Real-time Kanban + List project management for small teams.  
> Single binary. No Docker Required. No Postgres. No Redis. Just drop it and run.

---

## Why ZeroBoard?

Most self-hosted PM tools need Docker Compose, PostgreSQL, Redis, and a VPS with 2GB+ RAM.  
ZeroBoard runs as **one binary** on a **$5/mo VPS** and uses less than **80MB of RAM**.

---

## Quick Start

### Option A — Single Binary (Recommended)

```bash
# Download the latest binary
curl -L https://github.com/LogicLayerBD/zeroboard/releases/latest/download/zeroboard-linux-x86_64 -o zeroboard
chmod +x zeroboard

# Configure
cp .env.example .env
# Edit .env — at minimum, change JWT_SECRET

# Run
./zeroboard
```

Open `http://localhost:3000` — the first user to register becomes admin.

### Option B — Docker

Even in Docker, ZeroBoard runs as a single container with no external dependencies.

```bash
# Build the binary and image (on Linux x86_64 — the binary is copied into the image as-is)
make docker-build

# Configure
cp .env.example .env
# Edit .env — at minimum, change JWT_SECRET

# Persistent data dir, owned by the container's non-root user (UID 1000)
mkdir -p data/attachments && sudo chown -R 1000:1000 data

# Run
docker compose up -d
```

The SQLite database and attachments live in `./data` on the host.

> **Note:** The Docker image must be built on Linux x86_64. 
> On Mac or Windows, build on your VPS directly or use a CI pipeline.

---



## Features (v1)

- **Kanban view** — drag-and-drop cards between lists, real-time synced
- **List view** — sortable table view of all cards on a board
- **Card details** — description (Markdown), assignees, due dates, labels, file attachments, time tracking, comments
- **Real-time collaboration** — changes appear instantly for all team members
- **Multi-workspace** — separate workspaces with member roles (Admin, Member, Viewer)
- **Presence indicators** — see who's viewing the same board
- **In-app notifications** — assignment and due date alerts

---



## Tech Stack


| Layer      | Technology                        |
| ---------- | --------------------------------- |
| Backend    | Rust + Axum + Tokio               |
| Frontend   | Svelte 5 + Tailwind CSS           |
| Database   | SQLite (WAL mode)                 |
| Real-time  | WebSockets                        |
| Deployment | Single binary (frontend embedded) |


---



## Self-Hosting



### Requirements

- Linux x86_64 (Ubuntu 20.04+ recommended)
- 512MB RAM minimum
- 1GB disk space (for attachments)



### Environment Variables

See `.env.example` for all options. Required:

- `JWT_SECRET` — long random string (generate with `openssl rand -hex 32`)
- `DATABASE_URL` — defaults to `sqlite://data/zeroboard.db`



### Reverse Proxy (Nginx)

```nginx
server {
    listen 443 ssl;
    server_name yourboard.example.com;

    location / {
        proxy_pass http://localhost:3000;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";  # required for WebSockets
        proxy_set_header Host $host;
    }
}
```



### Systemd Service

```ini
[Unit]
Description=ZeroBoard
After=network.target

[Service]
Type=simple
User=www-data
WorkingDirectory=/opt/zeroboard
EnvironmentFile=/opt/zeroboard/.env
ExecStart=/opt/zeroboard/zeroboard
Restart=on-failure

[Install]
WantedBy=multi-user.target
```

---



## Building from Source

```bash
# Requirements: Rust 1.75+, Node.js 20+
git clone https://github.com/yourusername/zeroboard
cd zeroboard
make setup
make release
```

---



## License

MIT © LogicLayer