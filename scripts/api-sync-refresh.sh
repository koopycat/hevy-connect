#!/usr/bin/env bash
# Refreshes the Hevy API capture and records what the publish job should do.
#
# Runs without any write credential: it executes project code (cargo and
# just check) and reads third-party data. Its only output is a directory of
# plain files that the publish job copies from, so nothing here reaches the
# repository's write token.
set -euo pipefail

OUT="${1:?usage: api-sync-refresh.sh <output-dir>}"
mkdir -p "$OUT"

# Exits non-zero, writing nothing, when the live script cannot be read safely.
cargo run --locked --example api-sync | tee "$OUT/sync.log"

if git diff --quiet -- docs; then
  echo unchanged >"$OUT/state"
  exit 0
fi

check_result=passed
if ! just check >"$OUT/check.log" 2>&1; then
  check_result=failed
  echo "::group::just check output"
  cat "$OUT/check.log"
  echo "::endgroup::"
fi

echo changed >"$OUT/state"
echo "$check_result" >"$OUT/check"
cp docs/hevy-openapi.json "$OUT/hevy-openapi.json"

sha="$(sha256sum docs/hevy-openapi.json | cut -d' ' -f1)"
{
  cat <<EOF
The weekly check found that Hevy's live OpenAPI document differs from the committed capture.

## Contract changes

\`\`\`
$(grep -E '^  (  )?[-+~] |Only formatting' "$OUT/sync.log" || true)
\`\`\`

New capture SHA-256: \`$sha\`

## Checks

\`just check\` **$check_result** on this branch.

## Before merging

- [ ] Review the contract changes above against \`docs/hevy-api-analysis.md\`.
- [ ] Update the SHA and capture date in \`AGENTS.md\` and \`docs/hevy-api-analysis.md\`.
- [ ] Update the client or commands if the change alters behavior. A failing `MEASUREMENT_FIELDS` test means the measurement schema changed.

Edits pushed to this branch are kept: the next run builds on top of the branch rather than resetting it.
EOF
} >"$OUT/body.md"
