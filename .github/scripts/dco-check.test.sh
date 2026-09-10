#!/usr/bin/env bash
# Self-test for dco-check.sh. Builds throwaway git repos whose commits are signed off,
# unsigned, signed by the wrong address, or older than the rule, and asserts the guard's
# exit code. Pure bash + git.
#
#   bash .github/scripts/dco-check.test.sh
#
# A guard nobody tests is a guard that can pass everything and never say so: this repo
# shipped a smoke test that was broken from #58 until #71 without a single run noticing.
#
# Exit 0 when every case passes, 1 otherwise.

set -uo pipefail

SCRIPT="$(cd "$(dirname "$0")" && pwd)/dco-check.sh"

# Read the guard's own cutoff rather than repeating it, so moving the date (or dropping
# the grandfather clause) cannot leave these cases quietly testing the wrong thing.
EFFECTIVE="$(grep -m1 '^DCO_EFFECTIVE=' "$SCRIPT" | cut -d'"' -f2)"
BEFORE="2020-01-01" # comfortably before any cutoff
AFTER="2099-01-01"  # comfortably after any cutoff

PASS=0
FAIL=0
TMPDIRS=()

cleanup() {
	for d in "${TMPDIRS[@]:-}"; do [ -n "$d" ] && rm -rf "$d"; done
}
trap cleanup EXIT

fresh_repo() { # -> path to a repo with one root commit, signed off
	local dir
	dir="$(mktemp -d)"
	TMPDIRS+=("$dir")
	git -C "$dir" init -q -b main
	git -C "$dir" config user.email dev@example.com
	git -C "$dir" config user.name "A Dev"
	git -C "$dir" config commit.gpgsign false
	printf 'root\n' >"$dir/file.txt"
	git -C "$dir" add -A >/dev/null
	commit_at "$dir" "$AFTER" "root" "Signed-off-by: A Dev <dev@example.com>"
	printf '%s' "$dir"
}

commit_at() { # repo, YYYY-MM-DD, subject, [trailer...]
	local dir="$1" date="$2" subject="$3"
	shift 3
	local msg="$subject"
	local t
	for t in "$@"; do msg="$msg"$'\n\n'"$t"; done
	printf '%s\n' "$RANDOM$RANDOM" >>"$dir/file.txt"
	git -C "$dir" add -A >/dev/null
	GIT_AUTHOR_DATE="${date}T10:00:00+00:00" GIT_COMMITTER_DATE="${date}T10:00:00+00:00" \
		git -C "$dir" commit -q -m "$msg"
}

run() { (cd "$1" && bash "$SCRIPT" "$2" "$3" >/dev/null 2>&1); }

expect() { # description, expected_rc, actual_rc
	if [ "$2" = "$3" ]; then
		echo "ok   - $1"
		PASS=$((PASS + 1))
	else
		echo "FAIL - $1 (expected rc=$2, got rc=$3)"
		FAIL=$((FAIL + 1))
	fi
}

# 1. A signed-off commit passes.
r="$(fresh_repo)"
base="$(git -C "$r" rev-parse HEAD)"
commit_at "$r" "$AFTER" "seed: a change" "Signed-off-by: A Dev <dev@example.com>"
run "$r" "$base" HEAD
expect "signed-off commit -> passes" 0 $?

# 2. An unsigned commit fails.
r="$(fresh_repo)"
base="$(git -C "$r" rev-parse HEAD)"
commit_at "$r" "$AFTER" "seed: a change"
run "$r" "$base" HEAD
expect "commit with no Signed-off-by -> fails" 1 $?

# 3. Signed off by somebody other than the author fails.
r="$(fresh_repo)"
base="$(git -C "$r" rev-parse HEAD)"
commit_at "$r" "$AFTER" "seed: a change" "Signed-off-by: Someone Else <other@example.com>"
run "$r" "$base" HEAD
expect "signed off by a different address -> fails" 1 $?

# 4. The author's own line among several (a co-authored change) passes.
r="$(fresh_repo)"
base="$(git -C "$r" rev-parse HEAD)"
commit_at "$r" "$AFTER" "seed: a change" \
	"Signed-off-by: Someone Else <other@example.com>" \
	"Signed-off-by: A Dev <dev@example.com>"
run "$r" "$base" HEAD
expect "author's line among several sign-offs -> passes" 0 $?

# 5. Case in the address does not matter; the mailbox is the same.
r="$(fresh_repo)"
base="$(git -C "$r" rev-parse HEAD)"
commit_at "$r" "$AFTER" "seed: a change" "Signed-off-by: A Dev <Dev@Example.COM>"
run "$r" "$base" HEAD
expect "sign-off address differing only in case -> passes" 0 $?

# 6. A commit authored before the rule is exempt even unsigned.
r="$(fresh_repo)"
base="$(git -C "$r" rev-parse HEAD)"
commit_at "$r" "$BEFORE" "spec: old work"
run "$r" "$base" HEAD
expect "unsigned commit authored before the rule -> exempt, passes" 0 $?

# 7. An unsigned commit on the effective day itself is checked, not exempt (the
#    boundary is inclusive: the rule applies from that date, not after it).
r="$(fresh_repo)"
base="$(git -C "$r" rev-parse HEAD)"
commit_at "$r" "$EFFECTIVE" "seed: same-day work"
run "$r" "$base" HEAD
expect "unsigned commit authored on the effective date -> fails" 1 $?

# 8. Merge commits are GitHub's, not an author's, and are skipped.
r="$(fresh_repo)"
base="$(git -C "$r" rev-parse HEAD)"
git -C "$r" checkout -q -b topic
commit_at "$r" "$AFTER" "seed: on the branch" "Signed-off-by: A Dev <dev@example.com>"
git -C "$r" checkout -q main
GIT_AUTHOR_DATE="${AFTER}T11:00:00+00:00" GIT_COMMITTER_DATE="${AFTER}T11:00:00+00:00" \
	git -C "$r" merge -q --no-ff -m "Merge branch 'topic'" topic
run "$r" "$base" HEAD
expect "unsigned merge commit over signed work -> skipped, passes" 0 $?

# 9. A range with no commits in it passes.
r="$(fresh_repo)"
base="$(git -C "$r" rev-parse HEAD)"
run "$r" "$base" HEAD
expect "empty range -> passes" 0 $?

# 10. One unsigned commit among signed ones still fails the range.
r="$(fresh_repo)"
base="$(git -C "$r" rev-parse HEAD)"
commit_at "$r" "$AFTER" "seed: first" "Signed-off-by: A Dev <dev@example.com>"
commit_at "$r" "$AFTER" "seed: second"
commit_at "$r" "$AFTER" "seed: third" "Signed-off-by: A Dev <dev@example.com>"
run "$r" "$base" HEAD
expect "one unsigned commit among signed ones -> fails" 1 $?

# 11. Wrong number of arguments is a usage error, not a pass.
r="$(fresh_repo)"
(cd "$r" && bash "$SCRIPT" >/dev/null 2>&1)
expect "no arguments -> usage error" 2 $?

# 12. A base the clone cannot see is an error, not a silent pass. This is the shallow
#     checkout mistake: without fetch-depth: 0 the base sha is simply absent.
r="$(fresh_repo)"
run "$r" "0000000000000000000000000000000000000000" HEAD
expect "base sha not in the repository -> error" 2 $?

echo "-----"
echo "$PASS passed, $FAIL failed"
[ "$FAIL" -eq 0 ]
