#!/bin/sh
# Nightly Postgres backup for the single-VM deployment (MASTERPLAN M4.5).
# Takes a pg_dump (custom format, compressed) from the compose postgres
# service, validates the archive is readable, and prunes old dumps.
#
#   ./ops/pg-backup.sh                 # from the repo checkout on the VM
#
# Knobs (env):
#   BACKUP_DIR      where dumps land            (default /var/backups/gamma)
#   COMPOSE_FILE    compose file with postgres  (default compose.prod.yml)
#   RETENTION_DAYS  prune dumps older than this (default 14)
#
# Restore counterpart: ops/pg-restore.sh. Off-VM copy of $BACKUP_DIR is a
# separate, mandatory leg — see docs/OPERATIONS.md §7.
set -eu
# Dumps hold password hashes and the money journal — never world-readable,
# not even for the moment between creation and the chmod below.
umask 077

REPO_ROOT="$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)"
BACKUP_DIR="${BACKUP_DIR:-/var/backups/gamma}"
COMPOSE_FILE="${COMPOSE_FILE:-$REPO_ROOT/compose.prod.yml}"
RETENTION_DAYS="${RETENTION_DAYS:-14}"

STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
OUT="$BACKUP_DIR/gamma-$STAMP.dump"
TMP="$OUT.partial"

mkdir -p "$BACKUP_DIR"

# Leftover partials must not survive a failing night after night: clean up
# BEFORE the dump (a full disk would otherwise keep every future run failing),
# and clean up our own partial on any exit.
find "$BACKUP_DIR" -maxdepth 1 -type f -name 'gamma-*.dump.partial' \
  ! -path "$TMP" -exec rm -f {} \;
trap 'rm -f "$TMP"' EXIT

# Dump inside the container (peer auth on the local socket, no password needed);
# credentials come from the container's own POSTGRES_* environment.
docker compose -f "$COMPOSE_FILE" exec -T postgres sh -c \
  'exec pg_dump -U "${POSTGRES_USER:-gamma}" -d "${POSTGRES_DB:-gamma}" --format=custom' \
  > "$TMP"

# A dump that pg_restore cannot list is not a backup. Validate before renaming.
docker compose -f "$COMPOSE_FILE" exec -T postgres pg_restore --list \
  < "$TMP" > /dev/null

chmod 600 "$TMP"
mv "$TMP" "$OUT"
echo "backup ok: $OUT ($(du -h "$OUT" | cut -f1))"

# Prune this directory only (never dumps deliberately stashed in
# subdirectories), and never below one dump: a long outage must not silently
# delete the last good backup.
# shellcheck disable=SC2012  # filenames are machine-generated, ls -t is safe
KEEP="$(ls -1t "$BACKUP_DIR"/gamma-*.dump 2>/dev/null | head -1)"
if [ -n "$KEEP" ]; then
  find "$BACKUP_DIR" -maxdepth 1 -type f -name 'gamma-*.dump' \
    -mtime +"$RETENTION_DAYS" ! -name "$(basename "$KEEP")" -exec rm -f {} \;
fi
