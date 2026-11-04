#!/usr/bin/env bash
# Coverage gate for rsky-pds and rsky-oauth: every line a change adds or
# rewrites must be covered by the tests. The crate-wide floor is enforced
# separately by CI's cargo llvm-cov --fail-under-lines step.
#
#   scripts/coverage-gate.sh [<base-ref>]
#
# <base-ref> defaults to origin/main; the working tree (uncommitted edits
# included) is compared with its merge base, so the line numbers match the
# sources the tests were run against. Test files (tests/ and *_tests.rs) are
# not gated. Lines that carry no code of their own are not gated either:
# blank lines, comments, attributes, closing brackets, and struct or enum
# headers, which is where llvm attributes the parts of derive-generated code
# no test can reach (Rocket's FromForm error paths, askama's Display error
# mapping). Function coverage is reported but not gated, since it counts
# those generated functions and every per-binary instantiation of a closure.
# Needs cargo-llvm-cov and python3. With COVERAGE_GATE_REUSE_PROFILE set, the
# profile an earlier `cargo llvm-cov --no-report` left behind is reported
# instead of running the tests again.
set -euo pipefail
BASE=$(git merge-base "${1:-origin/main}" HEAD)
CRATES=(rsky-pds rsky-oauth)
OUT=$(mktemp -t rsky-cov.XXXXXX.txt)
trap 'rm -f "$OUT"' EXIT

args=()
for c in "${CRATES[@]}"; do args+=(-p "$c"); done
# Profiles left by an earlier run would be merged into this report, so the
# gate starts from a clean slate. Test failures are the test job's concern;
# the gate only needs the profile.
if [ -z "${COVERAGE_GATE_REUSE_PROFILE:-}" ]; then
  cargo llvm-cov clean --workspace >/dev/null 2>&1
  cargo llvm-cov --no-report --no-fail-fast "${args[@]}" >/dev/null 2>&1 \
    || echo "warning: some tests failed while profiling; coverage is reported anyway"
fi
cargo llvm-cov report --show-missing-lines "${args[@]}" > "$OUT" 2>/dev/null

CHANGED=$(git diff --name-only "$BASE" -- 'rsky-pds/src/**/*.rs' 'rsky-pds/src/*.rs' 'rsky-oauth/src/**/*.rs' 'rsky-oauth/src/*.rs' 2>/dev/null \
  | grep -vE '(^|/)tests?/|_tests?\.rs$' || true)

python3 - "$OUT" "$BASE" $CHANGED <<'PY'
import re, subprocess, sys
report, base, changed = sys.argv[1], sys.argv[2], sys.argv[3:]

# "Uncovered Lines:" lists `path: 1, 2, 3` per file; the table above it
# carries the per-file percentages.
uncovered, summary = {}, {}
in_missing = False
for line in open(report):
    if line.startswith("Uncovered Lines:"):
        in_missing = True
        continue
    if in_missing:
        m = re.match(r"^(\S+): (.*)$", line.strip())
        if m:
            uncovered[m.group(1)] = set()
            # newer cargo-llvm-cov prints runs of lines as ranges
            for item in m.group(2).split(", "):
                if not item:
                    continue
                first, _, last = item.partition("-")
                uncovered[m.group(1)].update(range(int(first), int(last or first) + 1))
        continue
    cols = line.split()
    if len(cols) >= 10 and cols[0].endswith(".rs"):
        summary[cols[0]] = (cols[9], cols[6])  # lines %, functions %

def touched_lines(path):
    diff = subprocess.run(
        ["git", "diff", "-U0", base, "--", path],
        capture_output=True, text=True, check=True).stdout
    lines = set()
    for m in re.finditer(r"^@@ -\d+(?:,\d+)? \+(\d+)(?:,(\d+))? @@", diff, re.M):
        start, count = int(m.group(1)), int(m.group(2) or 1)
        lines.update(range(start, start + count))
    return lines

NOT_CODE = re.compile(r"^\s*($|//|#!?\[|(pub(\(.*?\))?\s+)?(struct|enum)\s|[})\]]+[,;]?\s*$)")

def is_code(source, n):
    return n <= len(source) and not NOT_CODE.match(source[n - 1])

ok = True
for rel in changed:
    key = next((k for k in summary if k.endswith(rel)), None)
    if key is None:
        print(f"WARN: {rel} not in the coverage report (no instrumented code?)")
        continue
    source = open(rel).read().split("\n")
    missing = next((v for k, v in uncovered.items() if k.endswith(rel)), set())
    gated = sorted(n for n in touched_lines(rel) & missing if is_code(source, n))
    lines_pct, fns_pct = summary[key]
    status = "ok  " if not gated else "FAIL"
    if gated:
        ok = False
    print(f"{status} {rel}: file lines {lines_pct} functions {fns_pct}")
    if gated:
        print(f"     changed lines without coverage: {gated}")
if not changed:
    print("no changed source files to gate")
sys.exit(0 if ok else 1)
PY
