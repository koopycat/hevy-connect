#!/usr/bin/env bash
# Refreshes the Hevy API capture and opens, updates, or closes the pull request
# that carries the change. Run from the repository root by the Sync Hevy API
# workflow; it needs GH_TOKEN and a clean checkout of the default branch.
set -euo pipefail

readonly BRANCH="automated/hevy-api-sync"
readonly TITLE="chore: sync Hevy OpenAPI capture"
WORK_DIR="$(mktemp -d)"
readonly WORK_DIR
trap 'rm -rf "$WORK_DIR"' EXIT

# Exits non-zero, writing nothing, when the repairs cannot handle the live spec.
pnpm api:sync | tee "$WORK_DIR/sync.log"

existing_pr() {
  gh pr list --head "$BRANCH" --state open --json number --jq '.[0].number // empty'
}

if git diff --quiet -- docs src/generated; then
  pr="$(existing_pr)"
  if [ -n "$pr" ]; then
    gh pr close "$pr" \
      --comment "The live Hevy contract matches the committed capture again, so this pull request is closed." \
      --delete-branch
    echo "Closed #$pr: no remaining difference."
  else
    echo "No change to the Hevy contract."
  fi
  exit 0
fi

# A failing check does not block the PR: it becomes a draft so the breakage is
# visible without being merged by accident.
draft_flag=()
check_result="passed"
if ! pnpm check >"$WORK_DIR/check.log" 2>&1; then
  check_result="failed"
  draft_flag=(--draft)
fi

sha="$(sha256sum docs/hevy-openapi.json | cut -d' ' -f1)"

cat >"$WORK_DIR/body.md" <<EOF
The weekly check found that Hevy's live OpenAPI document differs from the committed capture.

## Contract changes

\`\`\`
$(grep -E '^  [-+~] |^Only formatting' "$WORK_DIR/sync.log" || true)
\`\`\`

New capture SHA-256: \`$sha\`

## Checks

\`pnpm check\` (the same suite as \`just check\`) **$check_result** on this branch.

## Before merging

- [ ] Review the contract changes above against \`docs/hevy-api-analysis.md\`.
- [ ] Update the SHA and capture date in \`AGENTS.md\` and \`docs/hevy-api-analysis.md\`.
- [ ] Update the client or commands if the change alters behavior.
EOF

git config user.name "github-actions[bot]"
git config user.email "41898282+github-actions[bot]@users.noreply.github.com"
git switch -q -C "$BRANCH"
git add docs/hevy-openapi.json src/generated/hevy-api.ts
git commit -q -m "$TITLE"
git push -q --force-with-lease -u origin "$BRANCH"

pr="$(existing_pr)"
if [ -n "$pr" ]; then
  gh pr edit "$pr" --title "$TITLE" --body-file "$WORK_DIR/body.md"
  echo "Updated #$pr."
else
  gh pr create --base main --head "$BRANCH" --title "$TITLE" \
    --body-file "$WORK_DIR/body.md" "${draft_flag[@]}"
fi
