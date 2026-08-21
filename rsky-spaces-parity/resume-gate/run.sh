#!/usr/bin/env bash
# Resume-across-swap gate: build the pre-convergence space host from a detached
# build-only worktree, run it with the real daemon, convert the store, then
# restart both the converged host and the same daemon and check what it
# projected. One command, no services beyond the child processes.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$REPO_ROOT"

# The legacy era is the pre-convergence tip of this branch and nothing else.
# Its worktree is build-only: no edit ever lands there.
LEGACY_REV="${LEGACY_REV:-6da61ce}"
LEGACY_TREE="${LEGACY_TREE:-/tmp/claude/rsky-resume-legacy}"
RUN_DIR="${RESUME_RUN_DIR:-$REPO_ROOT/target/resume/run}"

# sccache cannot open its cache in a sandboxed shell; the wrapper is only a
# build accelerator, so drop it rather than fail.
export RUSTC_WRAPPER=""
export CARGO_BUILD_RUSTC_WRAPPER=""

say() { printf '\n== %s\n' "$1"; }

LEGACY_SHA="$(git rev-parse "$LEGACY_REV")"

say "legacy worktree at $LEGACY_SHA"
if [ ! -d "$LEGACY_TREE/.git" ] && [ ! -f "$LEGACY_TREE/.git" ]; then
  mkdir -p "$(dirname "$LEGACY_TREE")"
  git worktree add --detach "$LEGACY_TREE" "$LEGACY_SHA"
fi
HAVE="$(git -C "$LEGACY_TREE" rev-parse HEAD)"
if [ "$HAVE" != "$LEGACY_SHA" ]; then
  echo "legacy worktree is at $HAVE, expected $LEGACY_SHA" >&2
  exit 1
fi
if [ -n "$(git -C "$LEGACY_TREE" status --porcelain)" ]; then
  echo "legacy worktree is dirty; it must stay build-only" >&2
  git -C "$LEGACY_TREE" status --short >&2
  exit 1
fi

say "building the legacy space host"
( cd "$LEGACY_TREE" && cargo build -p rsky-space-host --bin rsky-space-host )

say "building the converged space host, the converter and the daemon"
cargo build -p rsky-space-host --bin rsky-space-host --bin convert_store
cargo build -p rsky-daemon --bin rsky-daemon

say "building the gate"
cargo build -p rsky-spaces-parity --bin resume-gate

say "running the gate"
RESUME_RUN_DIR="$RUN_DIR" \
RESUME_LEGACY_SHIM_BIN="$LEGACY_TREE/target/debug/rsky-space-host" \
RESUME_SHIM_BIN="$REPO_ROOT/target/debug/rsky-space-host" \
RESUME_CONVERT_BIN="$REPO_ROOT/target/debug/convert_store" \
RESUME_DAEMON_BIN="$REPO_ROOT/target/debug/rsky-daemon" \
  ./target/debug/resume-gate
