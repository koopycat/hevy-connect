#!/usr/bin/env bash
# Publishes the refresh job's result: opens, updates, or closes the pull request
# for the automated branch.
#
# Runs with the write token and executes no project code. It works from the
# repository's default branch, never from a pull request's checkout.
set -euo pipefail

IN="${1:?usage: api-sync-publish.sh <refresh-output-dir>}"
readonly BRANCH="automated/hevy-api-sync"
readonly TITLE="chore: sync Hevy OpenAPI capture"
readonly COMPARE_URL="https://github.com/${GITHUB_REPOSITORY}/compare/main...${BRANCH}"

# Only pull requests from this repository: a fork can use any head branch name.
open_pr() {
  gh pr list --head "$BRANCH" --state open --json number,isCrossRepository \
    --jq '[.[] | select(.isCrossRepository == false)][0].number // empty'
}

state="$(cat "$IN/state")"
pr="$(open_pr)"

if [ "$state" = "unchanged" ]; then
  if [ -n "$pr" ]; then
    # The branch is kept, so any edits on it survive for the next difference.
    gh pr close "$pr" \
      --comment "The live Hevy contract matches the committed capture again, so this pull request is closed."
    echo "Closed #$pr: no remaining difference."
  else
    echo "No change to the Hevy contract."
  fi
  exit 0
fi

git config user.name "github-actions[bot]"
git config user.email "41898282+github-actions[bot]@users.noreply.github.com"

# Build on the remote branch when it exists, so earlier edits are kept and the
# push is a fast-forward. A shallow checkout has no remote-tracking ref, so
# fetch explicitly instead of relying on one.
if git fetch -q origin "$BRANCH" 2>/dev/null; then
  git switch -q -C "$BRANCH" FETCH_HEAD
else
  git switch -q -C "$BRANCH"
fi

cp "$IN/hevy-openapi.json" docs/hevy-openapi.json
cp "$IN/hevy-api.ts" src/generated/hevy-api.ts
git add docs/hevy-openapi.json src/generated/hevy-api.ts
if ! git diff --cached --quiet; then
  git commit -q -m "$TITLE"
fi
git push -q -u origin "$BRANCH"

check="$(cat "$IN/check" 2>/dev/null || echo failed)"
draft=()
if [ "$check" = failed ]; then
  draft=(--draft)
fi

if [ -n "$pr" ]; then
  gh pr edit "$pr" --title "$TITLE" --body-file "$IN/body.md"
  is_draft="$(gh pr view "$pr" --json isDraft --jq .isDraft)"
  if [ "$check" = failed ] && [ "$is_draft" = false ]; then
    gh pr ready "$pr" --undo
  elif [ "$check" = passed ] && [ "$is_draft" = true ]; then
    gh pr ready "$pr"
  fi
  echo "Updated #$pr."
else
  if ! gh pr create --base main --head "$BRANCH" --title "$TITLE" \
    --body-file "$IN/body.md" ${draft[@]+"${draft[@]}"}; then
    echo "::error::Could not open a pull request for $BRANCH. If GitHub reports that Actions cannot create pull requests, enable 'Allow GitHub Actions to create and approve pull requests' in the repository's Actions settings and rerun. The pushed branch is at $COMPARE_URL"
    exit 1
  fi
fi
