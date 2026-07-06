#!/bin/sh
# Restore a pg_dump archive into the compose postgres service (MASTERPLAN M4.5).
# DESTRUCTIVE: replaces the current database contents with the dump. Requires
# an explicit confirmation so it cannot fire by accident:
#
#   GAMMA_CONFIRM_RESTORE=yes ./ops/pg-restore.sh /var/backups/gamma/gamma-<stamp>.dump
#
# The public schema is RESET before restoring (not pg_restore --clean, which
# only drops objects that exist in the dump): after a bad deploy the live
# schema can be NEWER than the dump, and leftover objects would make core-api's
# embedded migration re-run fail ("relation already exists") in the middle of
# an incident. After the reset the database equals the dump exactly; if the
# code has moved on since, core-api applies the missing forward-only
# migrations on its next start. Works identically against a fresh empty
# volume (new VM).
#
# Sessions are wiped after the restore: a dump resurrects bearer tokens that
# were logged out or revoked since it was taken. Everyone logs in again.
# See docs/OPERATIONS.md §7 for the full procedure.
set -eu

REPO_ROOT="$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)"
COMPOSE_FILE="${COMPOSE_FILE:-$REPO_ROOT/compose.prod.yml}"
# Every service that holds DB connections; stopped during the restore.
APP_SERVICES="core-api transcode-worker settlement-scheduler ingestion"

DUMP="${1:?usage: GAMMA_CONFIRM_RESTORE=yes $0 <dump-file>}"
[ -f "$DUMP" ] || { echo "error: no such dump: $DUMP" >&2; exit 1; }
[ "${GAMMA_CONFIRM_RESTORE:-}" = "yes" ] || {
  echo "error: refusing to overwrite the database." >&2
  echo "Set GAMMA_CONFIRM_RESTORE=yes to proceed." >&2
  exit 1
}

# Remember what was actually running: a service the operator deliberately
# stopped before the incident must not come back just because we restored.
# "restarting" counts as running — a worker mid-crash-backoff (e.g. ingestion
# before its service account exists, OPERATIONS.md §3) is not "stopped".
RUNNING="$(docker compose -f "$COMPOSE_FILE" ps --services --status running --status restarting | tr '\n' ' ')"

echo "stopping app services (postgres stays up) ..."
# shellcheck disable=SC2086  # word splitting is the point
docker compose -f "$COMPOSE_FILE" stop $APP_SERVICES

echo "resetting schema and restoring $DUMP ..."
docker compose -f "$COMPOSE_FILE" exec -T postgres sh -c \
  'exec psql -U "${POSTGRES_USER:-gamma}" -d "${POSTGRES_DB:-gamma}" -q -v ON_ERROR_STOP=1 \
     -c "DROP SCHEMA public CASCADE; CREATE SCHEMA public;"'
docker compose -f "$COMPOSE_FILE" exec -T postgres sh -c \
  'exec pg_restore -U "${POSTGRES_USER:-gamma}" -d "${POSTGRES_DB:-gamma}" \
     --no-owner --exit-on-error' \
  < "$DUMP"

echo "invalidating restored sessions ..."
docker compose -f "$COMPOSE_FILE" exec -T postgres sh -c \
  'exec psql -U "${POSTGRES_USER:-gamma}" -d "${POSTGRES_DB:-gamma}" -q -v ON_ERROR_STOP=1 \
     -c "TRUNCATE sessions;"'

echo "starting the services that were running before ..."
# --no-build: their images necessarily exist locally (they WERE running) —
# never let compose silently build the checkout in the middle of an incident.
# shellcheck disable=SC2086  # word splitting is the point
docker compose -f "$COMPOSE_FILE" up -d --no-build $RUNNING

echo "restore ok. Verify: curl -s localhost:8080/health && curl -s localhost:8080/ready"
echo "note: operator actions taken after the dump (takedowns, verifications) are lost — re-check."
