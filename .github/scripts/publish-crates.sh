#!/usr/bin/env bash
# Publish every library crate whose current version is missing from crates.io,
# dependencies first. Pass --list to print the plan without publishing.
set -euo pipefail

CRATES=(rsky-common rsky-crypto rsky-syntax rsky-identity rsky-lexicon rsky-repo rsky-oauth rsky-space)
list_only=${1:-}

metadata=$(cargo metadata --no-deps --format-version 1)
selected=$(printf '%s\n' "${CRATES[@]}" | jq -R . | jq -s .)

# tsort prints dependencies before dependents; crates without edges are appended.
ordered=$(
  {
    jq -r --argjson c "$selected" '
      .packages[] | select(.name as $n | $c | index($n)) | .name as $n
      | .dependencies[] | select(.kind == null) | select(.name as $d | $c | index($d))
      | "\(.name) \($n)"' <<<"$metadata" | tsort
    printf '%s\n' "${CRATES[@]}"
  } | awk '!seen[$0]++'
)

for crate in $ordered; do
  version=$(jq -r --arg n "$crate" '.packages[] | select(.name == $n) | .version' <<<"$metadata")
  if curl -fsS -A "rsky-publish (github.com/blacksky-algorithms/rsky)" \
      "https://crates.io/api/v1/crates/$crate/$version" >/dev/null 2>&1; then
    echo "$crate $version is already on crates.io"
    continue
  fi
  echo "$crate $version needs publishing"
  [ "$list_only" = "--list" ] && continue
  cargo publish -p "$crate"
done
