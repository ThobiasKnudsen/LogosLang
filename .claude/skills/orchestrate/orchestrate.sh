#!/usr/bin/env bash
# The mechanical half of the /orchestrate skill: one worktree and branch per
# issue, one background Claude session per phase (worker, review round K),
# a serialized merge into dev from the main checkout; and one throwaway
# worktree per root-cause analysis (rca-SLUG), never merged. The skill decides; this runs.
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
CACHE_DIRS="${ORCH_CACHE_DIRS:-target}"
ORCH_HOME="${ORCH_HOME:-$HOME/.claude/orchestrate}"
QDIR="$ORCH_HOME/questions"
HDIR="$ORCH_HOME/history"

die() { echo "orchestrate: $*" >&2; exit 1; }
need() { command -v "$1" >/dev/null || die "missing tool: $1"; }
need claude; need jq; need gh; need git; need inotifywait

usage() {
  cat <<'USAGE'
usage: orchestrate.sh <command> [args]
  status                      every issue-* and rca-* session and worktree, one line each
  spawn-worker N slug         worktree + branch issue-N-slug from dev, start the worker
  spawn-reviewer N            start review round K+1 on the branch, stop every other issue-N-* session
  merge N                     merge the reviewed branch into dev, delete branch + worktree, close #N
  kill N                      stop and remove every issue-N-* session (worktree stays), take the
                              in-progress label off the issue and say where the branch stands
  spawn-rca SLUG              throwaway worktree rca-SLUG from dev, start the root-cause analysis
  spawn-rca-reviewer SLUG     start RCA review round K+1, stop every other rca-SLUG-* session
  rca-done SLUG               stop every rca-SLUG-* session, remove the worktree and branch (never merged)
  question-new PRIO PROJECT FILE
                              place FILE in the questions folder as PRIO_YYMMddHHmm_PROJECT.md;
                              {{ID}} in FILE becomes the question's id; prints the path
  question-prio ID PRIO       renumber an open question, links to it in the other open questions follow
  question-close ID           move an answered question to the history folder
  questions                   the open questions in the order they should be answered
  questions-watch             wait until an open question has an answer saved, print its path, exit
  agent-info ID               the session's model and context use, from its status line
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

branch_of() { git -C "$WT_ROOT/$1" branch --show-current 2>/dev/null || true; }
state_dir() { mkdir -p "$STATE_ROOT/$1"; echo "$STATE_ROOT/$1"; }
round_of() { local f; f=$(state_dir "$1")/round; [ -f "$f" ] && cat "$f" || echo 0; }
issue_title() { gh issue view "$1" --json title -q .title; }

# compose_prompt TEMPLATE TASK_PROMPT KEY=VALUE...: the template with every {{KEY}}
# replaced, then the task prompt.
compose_prompt() {
  local template=$1 task=$2; shift 2
  [ -f "$task" ] || die "write $task first (the task, why, reading list, files it touches)"
  local body kv
  body=$(cat "$template")
  for kv in "$@"; do body=${body//\{\{${kv%%=*}\}\}/${kv#*=}}; done
  printf '%s\n\n%s\n' "$body" "$(cat "$task")"
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

# a reflink copy of the main checkout's build: cargo then rebuilds the local crate only
warm_caches() {
  local wt=$1 d
  for d in $CACHE_DIRS; do
    [ -d "$REPO/$d" ] && [ ! -e "$wt/$d" ] && cp -a --reflink=auto "$REPO/$d" "$wt/$d"
  done
  return 0
}

# the merged worktree holds the newest build: its profile directories replace the main checkout's
keep_caches() {
  local wt=$1 d p
  for d in $CACHE_DIRS; do
    for p in release debug; do
      [ -d "$wt/$d/$p" ] || continue
      mkdir -p "$REPO/$d"
      rm -rf "$REPO/$d/$p"
      mv "$wt/$d/$p" "$REPO/$d/$p"
    done
  done
}

# worktree DIR BRANCH: create $WT_ROOT/DIR on BRANCH (existing or new from dev)
ensure_worktree() {
  local wt="$WT_ROOT/$1" branch=$2
  [ -d "$wt" ] && return
  if git -C "$REPO" show-ref --verify --quiet "refs/heads/$branch"; then
    git -C "$REPO" worktree add "$wt" "$branch"
  else
    git -C "$REPO" worktree add -b "$branch" "$wt" "$BASE"
  fi
  warm_caches "$wt"
}

cmd_status() {
  echo "sessions:"
  sessions_json | jq -r '.[] | select(.kind == "background" and ((.name // "") | test("^(issue|rca)-")))
    | "  \(.name)\t\(.id)\t\(.status)\t\(.state)\t\(.waitingFor // "")"' | sort
  echo "worktrees:"
  local wt d br ahead dirty
  for wt in "$WT_ROOT"/issue-*/ "$WT_ROOT"/rca-*/; do
    [ -d "$wt" ] || continue
    d=${wt%/}; d=${d##*/}
    br=$(branch_of "$d")
    ahead=$(git -C "$wt" rev-list --count "$BASE..HEAD" 2>/dev/null || echo '?')
    dirty=$(git -C "$wt" status --porcelain 2>/dev/null | wc -l)
    echo "  $d	$br	$ahead commits ahead of $BASE	$dirty uncommitted	review round $(round_of "$d")"
  done
}

mark_in_progress() {
  gh label create in-progress --color FBCA04 \
    --description "Being worked on now; the assignee and the latest comment say by whom" >/dev/null 2>&1 || true
  gh issue edit "$1" --add-assignee @me --add-label in-progress >/dev/null
}
unmark_in_progress() { gh issue edit "$1" --remove-label in-progress >/dev/null 2>&1 || true; }

cmd_pause() {
  local n=$1 wt="$WT_ROOT/issue-$1"
  cmd_kill "$n"
  unmark_in_progress "$n"
  [ -d "$wt" ] && gh issue comment "$n" --body "Paused: every session on this issue is stopped. Branch \`$(branch_of "issue-$n")\` stands at $(git -C "$wt" rev-parse --short HEAD) in \`.claude/worktrees/issue-$n\`." >/dev/null
  return 0
}

cmd_spawn_worker() {
  local n=$1 slug=$2 branch="issue-$1-$2" wt="$WT_ROOT/issue-$1"
  [ -z "$(ids_named "issue-$n-")" ] || die "a session is already on #$n; kill $n first"
  ensure_worktree "issue-$n" "$branch"
  branch=$(branch_of "issue-$n")
  local title prompt id
  title=$(issue_title "$n")
  prompt=$(compose_prompt "$SCRIPT_DIR/worker-prompt.md" "$HOME/.claude/plans/issue-$n-prompt.md" \
    "N=$n" "BRANCH=$branch" "WT=$wt" "REPO=$REPO" "ORCH=$ORCH" "ROUND=0" "TITLE=$title")
  printf '%s\n' "$prompt" > "$(state_dir "issue-$n")/worker.prompt.md"
  echo 0 > "$(state_dir "issue-$n")/round"
  id=$(start_session "$wt" "issue-$n-worker" "$prompt")
  echo "$id" > "$(state_dir "issue-$n")/worker.id"
  gh issue comment "$n" --body "Worker session \`issue-$n-worker\` ($id) started on branch \`$branch\` (worktree \`.claude/worktrees/issue-$n\`), orchestrated by \`$ORCH\`." >/dev/null
  mark_in_progress "$n"
  echo "worker for #$n: session $id, branch $branch, worktree $wt"
}

cmd_spawn_reviewer() {
  local n=$1 wt="$WT_ROOT/issue-$1" branch round title prompt id others
  [ -d "$wt" ] || die "no worktree for #$n"
  branch=$(branch_of "issue-$n")
  round=$(( $(round_of "issue-$n") + 1 ))
  title=$(issue_title "$n")
  prompt=$(compose_prompt "$SCRIPT_DIR/reviewer-prompt.md" "$HOME/.claude/plans/issue-$n-prompt.md" \
    "N=$n" "BRANCH=$branch" "WT=$wt" "REPO=$REPO" "ORCH=$ORCH" "ROUND=$round" "TITLE=$title")
  printf '%s\n' "$prompt" > "$(state_dir "issue-$n")/review-$round.prompt.md"
  others=$(ids_named "issue-$n-")
  id=$(start_session "$wt" "issue-$n-review-$round" "$prompt")
  [ "$(gh issue view "$n" --json state -q .state)" = "OPEN" ] && mark_in_progress "$n"
  echo "$round" > "$(state_dir "issue-$n")/round"
  echo "$id" > "$(state_dir "issue-$n")/review-$round.id"
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
  branch=$(branch_of "issue-$n")
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
  keep_caches "$wt"
  git -C "$REPO" worktree remove "$wt"
  git -C "$REPO" branch -d "$branch"
  if git -C "$REPO" ls-remote --exit-code --heads origin "$branch" >/dev/null 2>&1; then
    GIT_SSH_COMMAND="$GIT_SSH" git -C "$REPO" push origin --delete "$branch"
  fi
  GIT_SSH_COMMAND="$GIT_SSH" git -C "$REPO" push origin "$BASE"
  unmark_in_progress "$n"
  if [ "$(gh issue view "$n" --json state -q .state)" = "OPEN" ]; then
    gh issue close "$n" --comment "Merged to $BASE at $sha after review; branch \`$branch\` and its worktree deleted." >/dev/null
  else
    gh issue comment "$n" --body "Review branch \`$branch\` merged to $BASE at $sha; branch and worktree deleted." >/dev/null
  fi
  cmd_kill "$n" >/dev/null || true
  rm -rf "$STATE_ROOT/issue-$n"
  echo "merged #$n into $BASE at $sha; branch and worktree gone; issue closed"
}

cmd_spawn_rca() {
  local slug=$1 d="rca-$1" wt="$WT_ROOT/rca-$1" prompt id
  [ -z "$(ids_named "$d-")" ] || die "a session is already on rca $slug; rca-done $slug first"
  ensure_worktree "$d" "$d"
  prompt=$(compose_prompt "$SCRIPT_DIR/rca-prompt.md" "$HOME/.claude/plans/rca-$slug-prompt.md" \
    "SLUG=$slug" "BRANCH=$d" "WT=$wt" "REPO=$REPO" "ORCH=$ORCH" "ROUND=0")
  printf '%s\n' "$prompt" > "$(state_dir "$d")/worker.prompt.md"
  echo 0 > "$(state_dir "$d")/round"
  id=$(start_session "$wt" "$d-worker" "$prompt")
  echo "$id" > "$(state_dir "$d")/worker.id"
  echo "rca $slug: session $id, worktree $wt"
}

cmd_spawn_rca_reviewer() {
  local slug=$1 d="rca-$1" wt="$WT_ROOT/rca-$1" round prompt id others
  [ -d "$wt" ] || die "no worktree for rca $slug"
  round=$(( $(round_of "$d") + 1 ))
  prompt=$(compose_prompt "$SCRIPT_DIR/rca-reviewer-prompt.md" "$HOME/.claude/plans/rca-$slug-prompt.md" \
    "SLUG=$slug" "BRANCH=$d" "WT=$wt" "REPO=$REPO" "ORCH=$ORCH" "ROUND=$round")
  printf '%s\n' "$prompt" > "$(state_dir "$d")/review-$round.prompt.md"
  others=$(ids_named "$d-")
  id=$(start_session "$wt" "$d-review-$round" "$prompt")
  echo "$round" > "$(state_dir "$d")/round"
  echo "$id" > "$(state_dir "$d")/review-$round.id"
  for o in $others; do stop_rm "$o"; done
  echo "rca review round $round for $slug: session $id (stopped: ${others:-none})"
}

cmd_rca_done() {
  local slug=$1 d="rca-$1" wt="$WT_ROOT/rca-$1" ids
  ids=$(ids_named "$d-")
  for id in $ids; do stop_rm "$id"; echo "stopped $id"; done
  if [ -d "$wt" ]; then
    cp "$wt/RCA.md" "$(state_dir "$d")/RCA.md" 2>/dev/null || true
    git -C "$REPO" worktree remove --force "$wt"
  fi
  git -C "$REPO" show-ref --verify --quiet "refs/heads/$d" && git -C "$REPO" branch -D "$d" >/dev/null
  echo "rca $slug done; worktree and branch gone (RCA.md kept in $STATE_ROOT/$d)"
}

slug_of() { printf '%s' "$1" | tr '[:upper:]' '[:lower:]' | sed -E 's/[^a-z0-9]+/-/g; s/^-+|-+$//g'; }
question_file() { ls "$QDIR"/*.md 2>/dev/null | while read -r f; do grep -q "^# $1:" "$f" && { echo "$f"; break; }; done; }
answer_of() { awk '/^## Answer/ {f=1; next} f' "$1" | sed -E '/^<!--.*-->$/d; /^[[:space:]]*$/d'; }

cmd_question_new() {
  local prio=$1 project=$2 src=$3 n id stem name k tmp
  [ -f "$src" ] || die "no such file: $src"
  mkdir -p "$QDIR" "$HDIR" "$ORCH_HOME/tmp"
  n=$(( $(cat "$ORCH_HOME/next-id" 2>/dev/null || echo 0) + 1 )); echo "$n" > "$ORCH_HOME/next-id"
  id=$(printf 'Q-%04d' "$n")
  stem=$(printf '%02d_%s_%s' "$prio" "$(date +%y%m%d%H%M)" "$(slug_of "$project")")
  name="$stem.md"; k=2
  while [ -e "$QDIR/$name" ] || [ -e "$HDIR/$name" ]; do name="$stem-$k.md"; k=$((k + 1)); done
  tmp="$ORCH_HOME/tmp/$name"
  sed "s/{{ID}}/$id/g" "$src" > "$tmp"
  grep -q '^## Answer' "$tmp" || die "the question has no '## Answer' section"
  mv "$tmp" "$QDIR/$name"
  echo "$QDIR/$name"
}

cmd_question_prio() {
  local id=$1 prio=$2 f old new g
  f=$(question_file "$id"); [ -n "$f" ] || die "no open question $id"
  old=$(basename "$f"); new=$(printf '%02d_%s' "$prio" "${old#*_}")
  [ "$old" = "$new" ] && { echo "$f"; return; }
  [ -e "$QDIR/$new" ] && die "$new exists already; renumber that one first"
  sed -i -E "s/^(- \\*\\*Priority:\\*\\* )[0-9]+/\\1$prio/" "$f"
  mv "$f" "$QDIR/$new"
  for g in "$QDIR"/*.md; do [ -f "$g" ] && sed -i "s|$old|$new|g" "$g"; done
  echo "$QDIR/$new"
}

cmd_question_close() {
  local id=$1 f
  f=$(question_file "$id"); [ -n "$f" ] || die "no open question $id"
  mkdir -p "$HDIR"
  sed -i -E "s/^(- \\*\\*Status:\\*\\* ).*/\\1answered, relayed $(date '+%Y-%m-%d %H:%M')/" "$f"
  mv "$f" "$HDIR/$(basename "$f")"
  echo "$HDIR/$(basename "$f")"
}

cmd_questions() {
  local f
  for f in "$QDIR"/*.md; do
    [ -f "$f" ] || continue
    printf '%s\t%s\t%s\n' "$(basename "$f")" "$( [ -n "$(answer_of "$f")" ] && echo answered || echo open )" "$(head -1 "$f" | cut -c3-)"
  done
}

# exits only when an open question holds an answer, so the orchestrator's own writes never wake it
cmd_questions_watch() {
  mkdir -p "$QDIR"
  local f
  coproc INW { exec inotifywait -q -m -e close_write -e moved_to --format '%f' "$QDIR"; }
  sleep 0.3
  for f in "$QDIR"/*.md; do
    [ -f "$f" ] && [ -n "$(answer_of "$f")" ] && { kill "$INW_PID" 2>/dev/null; echo "$f"; return; }
  done
  while read -r f <&"${INW[0]}"; do
    case "$f" in *.md) ;; *) continue ;; esac
    [ -f "$QDIR/$f" ] && [ -n "$(answer_of "$QDIR/$f")" ] && { kill "$INW_PID" 2>/dev/null; echo "$QDIR/$f"; return; }
  done
}

cmd_agent_info() {
  claude logs "$1" 2>&1 \
    | sed -E 's/\x1b\[[0-9;?]*[a-zA-Z]//g; s/\x1b\][^\x07]*\x07//g; s/\r/\n/g' \
    | grep -oE '│ [^│]+ │ ctx [0-9]+%' | tail -1 | sed -E 's/^│ ([^│]+) │ ctx ([0-9]+%)$/model \1, context \2/'
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
  status)             cmd_status ;;
  spawn-worker)       [ $# -eq 3 ] || die "spawn-worker N slug"; cmd_spawn_worker "$2" "$3" ;;
  spawn-reviewer)     [ $# -eq 2 ] || die "spawn-reviewer N"; cmd_spawn_reviewer "$2" ;;
  merge)              [ $# -eq 2 ] || die "merge N"; cmd_merge "$2" ;;
  kill)               [ $# -eq 2 ] || die "kill N"; cmd_pause "$2" ;;
  spawn-rca)          [ $# -eq 2 ] || die "spawn-rca SLUG"; cmd_spawn_rca "$2" ;;
  spawn-rca-reviewer) [ $# -eq 2 ] || die "spawn-rca-reviewer SLUG"; cmd_spawn_rca_reviewer "$2" ;;
  rca-done)           [ $# -eq 2 ] || die "rca-done SLUG"; cmd_rca_done "$2" ;;
  question-new)       [ $# -eq 4 ] || die "question-new PRIO PROJECT FILE"; cmd_question_new "$2" "$3" "$4" ;;
  question-prio)      [ $# -eq 3 ] || die "question-prio ID PRIO"; cmd_question_prio "$2" "$3" ;;
  question-close)     [ $# -eq 2 ] || die "question-close ID"; cmd_question_close "$2" ;;
  questions)          cmd_questions ;;
  questions-watch)    cmd_questions_watch ;;
  agent-info)         [ $# -eq 2 ] || die "agent-info ID"; cmd_agent_info "$2" ;;
  attach)             [ $# -eq 2 ] || die "attach ID"; cmd_attach "$2" ;;
  logs)               [ $# -ge 2 ] || die "logs ID [LINES]"; cmd_logs "$2" "${3:-40}" ;;
  -h|--help|help|'')  usage ;;
  *) die "unknown command $1" ;;
esac
