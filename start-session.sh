#!/usr/bin/env bash
# DEPRECATED: kept as a compatibility wrapper for `winbridge start`.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BIN="$SCRIPT_DIR/target/release/winbridge"

if [[ -x "$BIN" ]]; then
    exec "$BIN" start
fi

cat <<'EOF'
winbridge Rust binary is not built yet.

Build it first:
  cargo build --release

Then run:
  ./target/release/winbridge start

For the legacy FreeRDP fallback, see README.md.
EOF
