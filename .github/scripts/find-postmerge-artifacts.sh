#!/usr/bin/env bash
set -euo pipefail

# Candidate retries reuse only their original run's smoke-checked uploads.
# Other reuse still requires a successful same-commit push to this fork's main.
repo="$GITHUB_REPOSITORY"
sha="$(git rev-parse HEAD)"
if [[ -n "${CANDIDATE_REF:-}" ]]; then
  [[ "$CANDIDATE_REF" =~ ^refs/heads/agent/release-([0-9]+)$ ]] || { echo 'Invalid candidate ref' >&2; exit 1; }
  candidate_run="${BASH_REMATCH[1]}"
  base=$(git rev-parse HEAD^)
  # Dispatch inputs are immutable on reruns, including which candidate is resumed.
  # Check this resume run before the original run to avoid rebuilding draft bytes.
  for id in "$GITHUB_RUN_ID" "$candidate_run"; do
    expected_base="$base"
    if [[ "$id" == "$GITHUB_RUN_ID" ]]; then expected_base="$GITHUB_SHA"; fi
    run=$(gh api "repos/$repo/actions/runs/$id")
    jq -e --arg base "$expected_base" --arg repo "$repo" '.head_sha == $base and .event == "workflow_dispatch" and .path == ".github/workflows/fork-rust-release.yml" and .repository.full_name == $repo and .head_repository.full_name == $repo' <<<"$run" >/dev/null
    artifacts=$(gh api "repos/$repo/actions/runs/$id/artifacts?per_page=100")
    if jq -e '[.artifacts[] | select(.expired == false) | .name] | (index("codex-aarch64-apple-darwin") != null and index("codex-x86_64-unknown-linux-gnu") != null)' <<<"$artifacts" >/dev/null; then
      echo "Reusing candidate run $id"
      echo 'reuse=true' >> "$GITHUB_OUTPUT"
      echo "run_id=$id" >> "$GITHUB_OUTPUT"
      exit 0
    fi
    if [[ "$GITHUB_RUN_ID" == "$candidate_run" ]]; then break; fi
  done
  # GITHUB_TOKEN pushes do not start postmerge CI for the candidate branch.
  echo 'reuse=false' >> "$GITHUB_OUTPUT"
  exit 0
fi
absent_deadline=$((SECONDS + 300))
deadline=$((SECONDS + 11100))
while :; do
  runs=$(gh api -X GET "repos/$repo/actions/workflows/postmerge-ci.yml/runs" -f head_sha="$sha" -f event=push -f per_page=100)
  runs_for_commit=$(jq -c --arg sha "$sha" --arg repo "$repo" '[.workflow_runs[] | select(.head_sha == $sha and .event == "push" and .head_branch == "main" and .repository.full_name == $repo and .head_repository.full_name == $repo)] | sort_by(.created_at) | reverse' <<<"$runs")
  while IFS= read -r run; do
    [[ -n "$run" ]] || continue
    [[ $(jq -r .status <<<"$run") == completed && $(jq -r .conclusion <<<"$run") == success ]] || continue
    id=$(jq -r .id <<<"$run")
    artifacts=$(gh api -X GET "repos/$repo/actions/runs/$id/artifacts?per_page=100")
    if jq -e '[.artifacts[] | select(.expired == false) | .name] | (index("codex-aarch64-apple-darwin") != null and index("codex-x86_64-unknown-linux-gnu") != null)' <<<"$artifacts" >/dev/null; then
      echo "Reusing successful postmerge run $id"
      echo 'reuse=true' >> "$GITHUB_OUTPUT"
      echo "run_id=$id" >> "$GITHUB_OUTPUT"
      exit 0
    fi
  done < <(jq -c '.[]' <<<"$runs_for_commit")

  active=$(jq -r '[.[] | select(.status != "completed") | .id] | first // empty' <<<"$runs_for_commit")
  if [[ -z "$active" ]]; then
    if [[ $(jq length <<<"$runs_for_commit") -eq 0 && $SECONDS -lt $absent_deadline ]]; then
      echo "Waiting briefly for postmerge run at $sha to appear"
      sleep 30
      continue
    fi
    echo 'No usable postmerge archives; release will build.'
    echo 'reuse=false' >> "$GITHUB_OUTPUT"
    exit 0
  fi
  if (( SECONDS >= deadline )); then
    echo "Timed out waiting for postmerge run $active" >&2
    exit 1
  fi
  echo "Waiting for postmerge run $active"
  sleep 60
done
