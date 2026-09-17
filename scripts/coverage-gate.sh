#!/usr/bin/env bash
# Coverage gate for rsky-pds and rsky-oauth: every source file changed since
# <base> must be 100% covered (lines and functions). The crate-wide floor is
# enforced separately by CI's cargo llvm-cov --fail-under-lines step.
#
#   scripts/coverage-gate.sh [<base-ref>]
#
# <base-ref> defaults to origin/main. Test files (tests/ and *_tests.rs) are
# not gated individually. Needs cargo-llvm-cov and python3.
set -euo pipefail
BASE=${1:-origin/main}
CRATES=(rsky-pds rsky-oauth)
OUT=$(mktemp -t rsky-cov.XXXXXX.json)
trap 'rm -f "$OUT"' EXIT

args=()
for c in "${CRATES[@]}"; do args+=(-p "$c"); done
# Profiles left by an earlier run would be merged into this report, so the
# gate starts from a clean slate. Test failures are the test job's concern;
# the gate only needs the profile.
cargo llvm-cov clean --workspace >/dev/null 2>&1
cargo llvm-cov --no-report --no-fail-fast "${args[@]}" >/dev/null 2>&1 \
  || echo "warning: some tests failed while profiling; coverage is reported anyway"
cargo llvm-cov report --json --output-path "$OUT" >/dev/null

CHANGED=$(git diff --name-only "$BASE"...HEAD -- 'rsky-pds/src/**/*.rs' 'rsky-pds/src/*.rs' 'rsky-oauth/src/**/*.rs' 'rsky-oauth/src/*.rs' 2>/dev/null \
  | grep -vE '(^|/)tests?/|_tests?\.rs$' || true)

python3 - "$OUT" $CHANGED <<'PY'
import json, sys, os
report, changed = sys.argv[1], sys.argv[2:]
data = json.load(open(report))["data"][0]
ok = True
by_suffix = {}
by_file = {}
for f in data["files"]:
    by_suffix[os.path.normpath(f["filename"])] = f["summary"]
    for rel in changed:
        if os.path.normpath(f["filename"]).endswith(rel):
            by_file[rel] = f
root = os.getcwd()
for rel in changed:
    summary = None
    for name, s in by_suffix.items():
        if name.endswith(rel):
            summary = s
            break
    if summary is None:
        print(f"WARN: {rel} not in the coverage report (no instrumented code?)")
        continue
    lp, fp = summary["lines"]["percent"], summary["functions"]["percent"]
    status = "ok  " if lp >= 100 and fp >= 100 else "FAIL"
    if status == "FAIL":
        ok = False
    print(f"{status} {rel}: lines {lp:.2f}% functions {fp:.2f}%")
    if lp < 100:
        missing = sorted({seg[0] for seg in by_file[rel]["segments"]
                          if seg[3] and seg[4] and seg[2] == 0})
        print(f"     uncovered lines: {missing}")
if not changed:
    print("no changed source files to gate")
sys.exit(0 if ok else 1)
PY
