#!/usr/bin/env bash
# Restores an archive made by backup.sh, replacing the database and attachments.
# The current data is archived first (zeroboard-pre-restore-*.tar.gz), so a
# restore can be undone by restoring that archive. Stops the systemd service
# for the swap and starts it again afterwards.
# Run it from the directory that holds .env and data/ (the service's WorkingDirectory).
#
# Usage: scripts/restore.sh <archive.tar.gz>
# Env:   ZEROBOARD_BIN  path to the binary     (default: backend/target/release/zeroboard)
#        SERVICE_NAME   systemd unit           (default: zeroboard)
#        BACKUP_DIR     where the pre-restore archive goes (default: backups)
set -euo pipefail
umask 077

readonly DEFAULT_BIN="backend/target/release/zeroboard"
readonly DEFAULT_SERVICE="zeroboard"
readonly DEFAULT_BACKUP_DIR="backups"
readonly DEFAULT_ATTACHMENTS_DIR="data/attachments"
readonly DEFAULT_DATABASE_URL="sqlite://data/zeroboard.db"
readonly PRE_RESTORE_LABEL="pre-restore"

die() { echo "error: $*" >&2; exit 1; }

env_value() {
  grep -E "^$1=" .env | tail -n 1 | cut -d= -f2- || true
}

as_root() {
  if [ "$(id -u)" -eq 0 ]; then "$@"; else sudo "$@"; fi
}

archive="${1:?usage: scripts/restore.sh <archive.tar.gz>}"
script_dir="$(cd "$(dirname "$0")" && pwd)"

[ -f .env ] || die "no .env in $(pwd); run from the ZeroBoard install directory"
[ -f "$archive" ] || die "archive not found: $archive"

bin="${ZEROBOARD_BIN:-$DEFAULT_BIN}"
service="${SERVICE_NAME:-$DEFAULT_SERVICE}"
backup_dir="${BACKUP_DIR:-$DEFAULT_BACKUP_DIR}"
attachments_dir="${ATTACHMENTS_DIR:-$(env_value ATTACHMENTS_DIR)}"
attachments_dir="${attachments_dir:-$DEFAULT_ATTACHMENTS_DIR}"
database_url="${DATABASE_URL:-$(env_value DATABASE_URL)}"
database_url="${database_url:-$DEFAULT_DATABASE_URL}"
db_path="${database_url#sqlite://}"
db_path="${db_path#sqlite:}"
db_path="${db_path%%\?*}"

[ -x "$bin" ] || die "binary not found at $bin (set ZEROBOARD_BIN)"

# Unpack and check the archive before touching anything. Staging sits next to
# the attachments dir so the final swap is a same-filesystem rename.
attachments_parent="$(dirname "$attachments_dir")"
mkdir -p "$attachments_parent" "$(dirname "$db_path")"
staging="$(mktemp -d "$attachments_parent/.restore-XXXXXX")"
service_stopped=false
cleanup() {
  rm -rf "$staging"
  if $service_stopped; then
    as_root systemctl start "$service" || echo "warning: failed to start $service" >&2
  fi
}
trap cleanup EXIT

tar -xzf "$archive" -C "$staging"
[ -f "$staging/zeroboard.db" ] && [ -d "$staging/attachments" ] \
  || die "$archive is not a ZeroBoard backup"

if command -v systemctl >/dev/null 2>&1 && systemctl is-active --quiet "$service"; then
  service_active=true
elif pgrep -x zeroboard >/dev/null 2>&1; then
  die "zeroboard is running outside systemd ($service); stop it first"
else
  service_active=false
fi

if [ -f "$db_path" ]; then
  echo "archiving current data before restore..."
  BACKUP_LABEL="$PRE_RESTORE_LABEL" KEEP_DAYS=0 ZEROBOARD_BIN="$bin" \
    ATTACHMENTS_DIR="$attachments_dir" "$script_dir/backup.sh" "$backup_dir"
fi

if $service_active; then
  echo "stopping $service..."
  as_root systemctl stop "$service"
  service_stopped=true
fi

# Verifies the backup, then swaps it in.
"$bin" restore --input "$staging/zeroboard.db"

if [ -e "$attachments_dir" ]; then
  mv "$attachments_dir" "$staging/attachments.replaced"
fi
mv "$staging/attachments" "$attachments_dir"

# Files created by root would be unwritable for the service user.
if [ "$(id -u)" -eq 0 ]; then
  owner="$(stat -c %u:%g "$attachments_parent")"
  chown -R "$owner" "$db_path" "$attachments_dir"
fi

echo "restored from $archive"
