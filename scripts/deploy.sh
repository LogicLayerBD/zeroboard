#!/usr/bin/env bash
# Updates a from-source install in place: backup, pull, build, restart, health check.
# Run it from the repository checkout that holds .env and data/ (the service's
# WorkingDirectory).
#
# Usage: scripts/deploy.sh
# Env:   SERVICE      systemd unit to restart           (default: zeroboard)
#        SKIP_BACKUP  set to 1 to skip the pre-deploy backup
set -euo pipefail

readonly DEFAULT_SERVICE="zeroboard"
readonly BIN="backend/target/release/zeroboard"
readonly DEFAULT_HOST="127.0.0.1"
readonly DEFAULT_PORT="3000"
# The server needs a moment to run migrations and bind; poll /ready for up to 30s.
readonly READY_ATTEMPTS=30
readonly READY_INTERVAL_SECS=1

die() { echo "error: $*" >&2; exit 1; }
step() { echo; echo "==> $*"; }

# Reads KEY from .env without executing the file.
env_value() {
  grep -E "^$1=" .env | tail -n 1 | cut -d= -f2- || true
}

[ -f .env ] || die "no .env in $(pwd); run from the ZeroBoard install directory"
service="${SERVICE:-$DEFAULT_SERVICE}"

changes="$(git status --porcelain --untracked-files=no)"
if [ -n "$changes" ]; then
  echo "$changes" >&2
  die "local changes to tracked files (discard with: git checkout -- <file>)"
fi

previous_commit="$(git rev-parse --short HEAD)"

if [ "${SKIP_BACKUP:-0}" != "1" ] && [ -x "$BIN" ]; then
  step "Backing up (migrations may run on restart)"
  # KEEP_DAYS=0: a deploy never deletes older backups.
  BACKUP_LABEL=pre-deploy KEEP_DAYS=0 scripts/backup.sh
fi

step "Pulling latest code"
git pull --ff-only
current_commit="$(git rev-parse --short HEAD)"
if [ "$current_commit" = "$previous_commit" ]; then
  echo "already up to date at $current_commit; rebuilding anyway"
fi

step "Building release binary"
make release

step "Restarting $service"
sudo systemctl restart "$service"

# 0.0.0.0 means "all interfaces"; probe through loopback instead.
host="$(env_value HOST)"
host="${host:-$DEFAULT_HOST}"
[ "$host" = "0.0.0.0" ] && host="$DEFAULT_HOST"
port="$(env_value PORT)"
port="${port:-$DEFAULT_PORT}"
ready_url="http://$host:$port/ready"

step "Waiting for $ready_url"
for _ in $(seq "$READY_ATTEMPTS"); do
  if curl -fsS --max-time "$READY_INTERVAL_SECS" "$ready_url" >/dev/null 2>&1; then
    echo
    echo "✅ Deployed $previous_commit → $current_commit; $service is ready."
    exit 0
  fi
  sleep "$READY_INTERVAL_SECS"
done

echo "❌ $service did not become ready. Recent logs:" >&2
sudo journalctl -u "$service" -n 30 --no-pager >&2 || true
echo >&2
echo "To roll back: git checkout $previous_commit && make release && sudo systemctl restart $service" >&2
echo "If a migration ran, restore the pre-deploy archive with scripts/restore.sh." >&2
exit 1
