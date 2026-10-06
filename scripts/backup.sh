#!/usr/bin/env bash
# Backs up the ZeroBoard database and attachments into one timestamped archive.
# Safe to run while the server is running. Run it from the directory that holds
# .env and data/ (the service's WorkingDirectory).
#
# Usage: scripts/backup.sh [backup_dir]        (default: backups)
# Env:   ZEROBOARD_BIN  path to the binary     (default: backend/target/release/zeroboard)
#        KEEP_DAYS      delete archives older than this many days; 0 keeps all (default: 14)
#        BACKUP_LABEL   optional tag added to the archive name (e.g. pre-restore)
set -euo pipefail
# Archives contain password hashes and session tokens.
umask 077

readonly DEFAULT_BIN="backend/target/release/zeroboard"
readonly DEFAULT_BACKUP_DIR="backups"
readonly DEFAULT_ATTACHMENTS_DIR="data/attachments"
readonly DEFAULT_KEEP_DAYS=14
readonly ARCHIVE_PREFIX="zeroboard-"
readonly ARCHIVE_SUFFIX=".tar.gz"

die() { echo "error: $*" >&2; exit 1; }

# Reads KEY from .env without executing the file.
env_value() {
  grep -E "^$1=" .env | tail -n 1 | cut -d= -f2- || true
}

[ -f .env ] || die "no .env in $(pwd); run from the ZeroBoard install directory"

backup_dir="${1:-$DEFAULT_BACKUP_DIR}"
bin="${ZEROBOARD_BIN:-$DEFAULT_BIN}"
keep_days="${KEEP_DAYS:-$DEFAULT_KEEP_DAYS}"
attachments_dir="${ATTACHMENTS_DIR:-$(env_value ATTACHMENTS_DIR)}"
attachments_dir="${attachments_dir:-$DEFAULT_ATTACHMENTS_DIR}"
label="${BACKUP_LABEL:+$BACKUP_LABEL-}"

[ -x "$bin" ] || die "binary not found at $bin (set ZEROBOARD_BIN)"
[[ "$keep_days" =~ ^[0-9]+$ ]] || die "KEEP_DAYS must be a whole number"

mkdir -p "$backup_dir"
archive="$backup_dir/${ARCHIVE_PREFIX}${label}$(date +%Y%m%d-%H%M%S)${ARCHIVE_SUFFIX}"
staging="$(mktemp -d)"
trap 'rm -rf "$staging" "$archive.partial"' EXIT

# Snapshot the database first: attachments uploaded after this point are
# harmless extras in the archive.
"$bin" backup --output "$staging/zeroboard.db"

if [ -d "$attachments_dir" ]; then
  ln -s "$(cd "$attachments_dir" && pwd)" "$staging/attachments"
else
  mkdir "$staging/attachments"
fi

# -h archives the symlink's target, so every archive holds a real attachments/ dir
# regardless of ATTACHMENTS_DIR's name.
tar -czhf "$archive.partial" -C "$staging" zeroboard.db attachments
mv "$archive.partial" "$archive"
echo "backup written: $archive ($(du -h "$archive" | cut -f1))"

if [ "$keep_days" -gt 0 ]; then
  find "$backup_dir" -maxdepth 1 -type f -name "${ARCHIVE_PREFIX}*${ARCHIVE_SUFFIX}" \
    -mtime +"$keep_days" -print -delete | sed 's/^/deleted old backup: /'
fi
