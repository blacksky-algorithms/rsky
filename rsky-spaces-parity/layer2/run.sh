#!/usr/bin/env bash
# Layer 2 acceptance gate: build both real binaries, run the same record script
# at each over XRPC, then compare the two store files with the Layer 1
# comparator. One command, no services beyond the two child processes.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$REPO_ROOT"

# The oracle is rsky-pds at this revision and nothing else. Its worktree is
# build-only: no edit ever lands there.
ORACLE_REV="${ORACLE_REV:-7ebd21ae788c550ee8510034d94eb19ede148738}"
ORACLE_TREE="${ORACLE_TREE:-/tmp/claude/rsky-layer2-oracle}"
RUN_DIR="${LAYER2_RUN_DIR:-$REPO_ROOT/target/layer2/run}"

# sccache cannot open its cache in a sandboxed shell; the wrapper is only a
# build accelerator, so drop it rather than fail.
export RUSTC_WRAPPER=""
export CARGO_BUILD_RUSTC_WRAPPER=""

say() { printf '\n== %s\n' "$1"; }

say "oracle worktree at $ORACLE_REV"
if [ ! -d "$ORACLE_TREE/.git" ] && [ ! -f "$ORACLE_TREE/.git" ]; then
  mkdir -p "$(dirname "$ORACLE_TREE")"
  git worktree add --detach "$ORACLE_TREE" "$ORACLE_REV"
fi
HAVE="$(git -C "$ORACLE_TREE" rev-parse HEAD)"
if [ "$HAVE" != "$ORACLE_REV" ]; then
  echo "oracle worktree is at $HAVE, expected $ORACLE_REV" >&2
  exit 1
fi
if [ -n "$(git -C "$ORACLE_TREE" status --porcelain)" ]; then
  echo "oracle worktree is dirty; it must stay build-only" >&2
  git -C "$ORACLE_TREE" status --short >&2
  exit 1
fi

say "building the pinned oracle pds"
( cd "$ORACLE_TREE" && cargo build -p rsky-pds --bin rsky-pds )

say "building the space host under test"
cargo build -p rsky-space-host --bin rsky-space-host

say "building the gate"
cargo build -p rsky-spaces-parity --bin layer2-gate

say "running the gate"
LAYER2_RUN_DIR="$RUN_DIR" \
LAYER2_PDS_BIN="$ORACLE_TREE/target/debug/rsky-pds" \
LAYER2_SHIM_BIN="$REPO_ROOT/target/debug/rsky-space-host" \
  ./target/debug/layer2-gate
