#!/usr/bin/env bash
# The mechanical half of the /orchestrate skill: one worktree and branch per
# issue, one background Claude session per phase (worker, review round K),
# a serialized merge into dev from the main checkout. The skill decides; this runs.
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO=$(git -C "$SCRIPT_DIR" worktree list --porcelain | awk 'NR==1 {print $2}')
WT_ROOT="$REPO/.claude/worktrees"
STATE_ROOT="${XDG_STATE_HOME:-$HOME/.local/state}/logos-orchestrate"
ORCH="${ORCH:-orchestrator}"
MODE="${ORCH_MODE:-auto}"
BASE=dev
TERMINAL_CMD="${ORCH_TERMINAL:-foot -e}"
GIT_SSH="${ORCH_GIT_SSH:-ssh -i $HOME/.ssh/id_ed25519_personal}"

die() { echo "orchestrate: $*" >&2; exit 1; }
need() { command -v "$1" >/dev/null || die "missing tool: $1"; }
need claude; need jq; need gh; need git

usage() {
  cat <<'USAGE'
usage: orchestrate.sh <command> [args]
  status                      every issue-* session and worktree, one line each
  spawn-worker N slug         worktree + branch issue-N-slug from dev, start the worker
  spawn-reviewer N            start review round K+1 on the branch, stop every other issue-N-* session
  merge N                     merge the reviewed branch into dev, delete branch + worktree, close #N
  kill N                      stop and remove every issue-N-* session (worktree stays)
  attach ID                   open the session in a new terminal window (Sway)
  logs ID [LINES]             the session's recent output, escape codes stripped
Environment: ORCH (orchestrator session name, default orchestrator), ORCH_MODE (workers'
permission mode, must equal the orchestrator's, default auto), ORCH_TERMINAL (default "foot -e").
USAGE
}

sessions_json() { claude agents --json 2>/dev/null || echo '[]'; }

# ids of live background sessions whose name starts with $1
ids_named() {
  sessions_json | jq -r --arg p "$1" \
    '.[] | select(.kind == "background" and ((.name // "") | startswith($p))) | .id'
}

stop_rm() {
  local id=$1
  claude stop "$id" >/dev/null 2>&1 || true
  claude rm "$id" >/dev/null 2>&1 || echo "orchestrate: could not rm $id (still listed)" >&2
}

branch_of() { git -C "$WT_ROOT/issue-$1" branch --show-current 2>/dev/null || true; }
state_dir() { mkdir -p "$STATE_ROOT/issue-$1"; echo "$STATE_ROOT/issue-$1"; }
round_of() { local f; f=$(state_dir "$1")/round; [ -f "$f" ] && cat "$f" || echo 0; }
issue_title() { gh issue view "$1" --json title -q .title; }

compose_prompt() {
  local template=$1 n=$2 branch=$3 round=$4 title=$5
  local issue_prompt="$HOME/.claude/plans/issue-$n-prompt.md"
  [ -f "$issue_prompt" ] || die "write $issue_prompt first (the task, why, reading list, files it touches)"
  local body
  body=$(cat "$template")
  body=${body//\{\{N\}\}/$n}
  body=${body//\{\{BRANCH\}\}/$branch}
  body=${body//\{\{WT\}\}/$WT_ROOT/issue-$n}
  body=${body//\{\{REPO\}\}/$REPO}
  body=${body//\{\{ORCH\}\}/$ORCH}
  body=${body//\{\{ROUND\}\}/$round}
  body=${body//\{\{TITLE\}\}/$title}
  printf '%s\n\n%s\n' "$body" "$(cat "$issue_prompt")"
}

# start a background session in the worktree; prints its id. The prompt goes
# before --disallowedTools because that flag is variadic and would swallow it.
# CLAUDE* variables of the calling session are dropped: inherited, they make
# the new session a child of the caller, and a child has no SendMessage tool.
start_session() {
  local wt=$1 name=$2 prompt=$3 out id
  local -a unset_args=()
  while IFS= read -r v; do unset_args+=(-u "$v"); done < <(env | grep -oE '^CLAUDE[A-Z0-9_]*')
  out=$(cd "$wt" && env "${unset_args[@]}" claude --bg -n "$name" --permission-mode "$MODE" "$prompt" --disallowedTools AskUserQuestion 2>&1) \
    || die "claude --bg failed: $out"
  id=$(printf '%s\n' "$out" | awk '/^backgrounded/ {print $3; exit}')
  [ -n "$id" ] || die "no session id in: $out"
  echo "$id"
}

cmd_status() {
  echo "sessions:"
  sessions_json | jq -r '.[] | select(.kind == "background" and ((.name // "") | startswith("issue-")))
    | "  \(.name)\t\(.id)\t\(.status)\t\(.state)\t\(.waitingFor // "")"' | sort
  echo "worktrees:"
  local wt n br ahead dirty
  for wt in "$WT_ROOT"/issue-*/; do
    [ -d "$wt" ] || continue
    n=${wt%/}; n=${n##*/issue-}
    br=$(branch_of "$n")
    ahead=$(git -C "$wt" rev-list --count "$BASE..HEAD" 2>/dev/null || echo '?')
    dirty=$(git -C "$wt" status --porcelain 2>/dev/null | wc -l)
    echo "  issue-$n	$br	$ahead commits ahead of $BASE	$dirty uncommitted	review round $(round_of "$n")"
  done
}

cmd_spawn_worker() {
  local n=$1 slug=$2 branch="issue-$1-$2" wt="$WT_ROOT/issue-$1"
  [ -z "$(ids_named "issue-$n-")" ] || die "a session is already on #$n; kill $n first"
  if [ ! -d "$wt" ]; then
    if git -C "$REPO" show-ref --verify --quiet "refs/heads/$branch"; then
      git -C "$REPO" worktree add "$wt" "$branch"
    else
      git -C "$REPO" worktree add -b "$branch" "$wt" "$BASE"
    fi
  fi
  branch=$(branch_of "$n")
  local title prompt id
  title=$(issue_title "$n")
  prompt=$(compose_prompt "$SCRIPT_DIR/worker-prompt.md" "$n" "$branch" 0 "$title")
  printf '%s\n' "$prompt" > "$(state_dir "$n")/worker.prompt.md"
  echo 0 > "$(state_dir "$n")/round"
  id=$(start_session "$wt" "issue-$n-worker" "$prompt")
  echo "$id" > "$(state_dir "$n")/worker.id"
  gh issue comment "$n" --body "Worker session started on branch \`$branch\` (worktree \`.claude/worktrees/issue-$n\`), orchestrated by \`$ORCH\`." >/dev/null
  echo "worker for #$n: session $id, branch $branch, worktree $wt"
}

cmd_spawn_reviewer() {
  local n=$1 wt="$WT_ROOT/issue-$1" branch round title prompt id others
  [ -d "$wt" ] || die "no worktree for #$n"
  branch=$(branch_of "$n")
  round=$(( $(round_of "$n") + 1 ))
  title=$(issue_title "$n")
  prompt=$(compose_prompt "$SCRIPT_DIR/reviewer-prompt.md" "$n" "$branch" "$round" "$title")
  printf '%s\n' "$prompt" > "$(state_dir "$n")/review-$round.prompt.md"
  others=$(ids_named "issue-$n-")
  id=$(start_session "$wt" "issue-$n-review-$round" "$prompt")
  echo "$round" > "$(state_dir "$n")/round"
  echo "$id" > "$(state_dir "$n")/review-$round.id"
  for o in $others; do stop_rm "$o"; done
  echo "review round $round for #$n: session $id (stopped: ${others:-none})"
}

cmd_kill() {
  local n=$1 ids
  ids=$(ids_named "issue-$n-")
  [ -n "$ids" ] || { echo "no live session on #$n"; return; }
  for id in $ids; do stop_rm "$id"; echo "stopped $id"; done
}

cmd_merge() {
  local n=$1 wt="$WT_ROOT/issue-$1" branch title sha
  [ -d "$wt" ] || die "no worktree for #$n"
  branch=$(branch_of "$n")
  [ "$(git -C "$REPO" branch --show-current)" = "$BASE" ] || die "main checkout is not on $BASE"
  [ -z "$(git -C "$wt" status --porcelain)" ] || die "worktree of #$n has uncommitted changes"
  GIT_SSH_COMMAND="$GIT_SSH" git -C "$REPO" fetch origin "$BASE"
  if ! git -C "$REPO" merge-base --is-ancestor "origin/$BASE" "$BASE"; then
    GIT_SSH_COMMAND="$GIT_SSH" git -C "$REPO" pull --ff-only --autostash origin "$BASE" \
      || die "local $BASE and origin/$BASE have diverged; resolve by hand"
  fi
  if ! git -C "$REPO" merge-base --is-ancestor "$BASE" "$branch"; then
    echo "orchestrate: $BASE moved since the review of #$n; run spawn-reviewer $n again" >&2
    exit 3
  fi
  title=$(issue_title "$n")
  git -C "$REPO" merge --no-ff --signoff "$branch" -m "Merge branch '$branch' into $BASE: $title; closes #$n"
  sha=$(git -C "$REPO" rev-parse --short HEAD)
  git -C "$REPO" worktree remove "$wt"
  git -C "$REPO" branch -d "$branch"
  if git -C "$REPO" ls-remote --exit-code --heads origin "$branch" >/dev/null 2>&1; then
    GIT_SSH_COMMAND="$GIT_SSH" git -C "$REPO" push origin --delete "$branch"
  fi
  GIT_SSH_COMMAND="$GIT_SSH" git -C "$REPO" push origin "$BASE"
  gh issue close "$n" --comment "Merged to $BASE at $sha after review; branch \`$branch\` and its worktree deleted." >/dev/null
  cmd_kill "$n" >/dev/null || true
  rm -rf "$STATE_ROOT/issue-$n"
  echo "merged #$n into $BASE at $sha; branch and worktree gone; issue closed"
}

cmd_attach() {
  local id=$1
  if command -v swaymsg >/dev/null && [ -n "${SWAYSOCK:-}" ]; then
    swaymsg exec "$TERMINAL_CMD claude attach $id" >/dev/null
    echo "opened a window on $id"
  else
    echo "no Sway; run: claude attach $id"
  fi
}

cmd_logs() {
  claude logs "$1" 2>&1 \
    | sed -E 's/\x1b\[[0-9;?]*[a-zA-Z]//g; s/\x1b\][^\x07]*\x07//g; s/\r//g' \
    | grep -v '^\s*$' | tail -n "${2:-40}"
}

case "${1:-}" in
  status)         cmd_status ;;
  spawn-worker)   [ $# -eq 3 ] || die "spawn-worker N slug"; cmd_spawn_worker "$2" "$3" ;;
  spawn-reviewer) [ $# -eq 2 ] || die "spawn-reviewer N"; cmd_spawn_reviewer "$2" ;;
  merge)          [ $# -eq 2 ] || die "merge N"; cmd_merge "$2" ;;
  kill)           [ $# -eq 2 ] || die "kill N"; cmd_kill "$2" ;;
  attach)         [ $# -eq 2 ] || die "attach ID"; cmd_attach "$2" ;;
  logs)           [ $# -ge 2 ] || die "logs ID [LINES]"; cmd_logs "$2" "${3:-40}" ;;
  -h|--help|help|'') usage ;;
  *) die "unknown command $1" ;;
esac
