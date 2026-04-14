#!/usr/bin/env bash
set -euo pipefail

if [ "$#" -gt 0 ]; then
  exec "$@"
fi

: "${OMNIFS_MOUNT_POINT:=/github}"
: "${OMNIFS_CONFIG_DIR:=/root/.omnifs}"
: "${OMNIFS_CACHE_DIR:=/tmp/omnifs-cache}"
: "${OMNIFS_LOG_FILE:=/tmp/omnifs.log}"
: "${RUST_LOG:=info}"
export RUST_LOG

mkdir -p \
  "$OMNIFS_MOUNT_POINT" \
  "$OMNIFS_CONFIG_DIR" \
  "$OMNIFS_CACHE_DIR" \
  "$(dirname "$OMNIFS_LOG_FILE")"

log_pipe=/tmp/omnifs-entrypoint.log.pipe
rm -f "$log_pipe"
mkfifo "$log_pipe"
tee -a "$OMNIFS_LOG_FILE" < "$log_pipe" &
exec >"$log_pipe" 2>&1
rm -f "$log_pipe"

exec omnifs mount \
  --mount-point "$OMNIFS_MOUNT_POINT" \
  --config-dir "$OMNIFS_CONFIG_DIR" \
  --cache-dir "$OMNIFS_CACHE_DIR"
