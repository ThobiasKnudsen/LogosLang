#!/usr/bin/env bash
# Self-test for docs-check.sh. Builds throwaway git repos with docs + tags and
# asserts the guard's exit code. Pure bash + git, no Node/TypeScript.
#
#   bash .github/scripts/docs-check.test.sh
#
# Exit 0 when every case passes, 1 otherwise.

set -uo pipefail

SCRIPT="$(cd "$(dirname "$0")" && pwd)/docs-check.sh"
PASS=0
FAIL=0
TMPDIRS=()

cleanup() {
	for d in "${TMPDIRS[@]:-}"; do [ -n "$d" ] && rm -rf "$d"; done
}
trap cleanup EXIT

# Fresh git repo with docs/, isolated so cases can't bleed into each other.
fresh_repo() {
	local dir
	dir="$(mktemp -d)"
	TMPDIRS+=("$dir")
	git -C "$dir" init -q
	git -C "$dir" config user.email t@example.com
	git -C "$dir" config user.name tester
	git -C "$dir" config commit.gpgsign false
	mkdir -p "$dir/docs/reference"
	printf '%s' "$dir"
}

doc() { # repo, relpath-under-docs, body
	local full="$1/docs/$2"
	mkdir -p "$(dirname "$full")"
	printf '# %s\n' "$3" >"$full"
}

commit() { git -C "$1" add -A >/dev/null; git -C "$1" commit -qm "$2"; }

# run <repo> <args...> ; returns the guard's exit code
run() { (cd "$1" && shift && bash "$SCRIPT" "$@" >/dev/null 2>&1); }

expect() { # description, expected_rc, actual_rc
	if [ "$2" = "$3" ]; then
		echo "ok   - $1"
		PASS=$((PASS + 1))
	else
		echo "FAIL - $1 (expected rc=$2, got rc=$3)"
		FAIL=$((FAIL + 1))
	fi
}

# 1. No tags, valid prefixes -> validate passes (nothing is frozen yet).
r="$(fresh_repo)"
doc "$r" reference/v0.0.1_operators.md "operators v1"
commit "$r" init
run "$r" validate
expect "no tags + valid prefixes -> validate passes" 0 $?

# 2. A file without a valid version prefix -> validate fails.
r="$(fresh_repo)"
doc "$r" reference/v0.0.1_operators.md "operators v1"
doc "$r" reference/notes.md "stray"
commit "$r" init
run "$r" validate
expect "unversioned .md -> validate fails" 1 $?

# 3. Freeze: with R=v0.0.1, modifying the released doc -> validate fails.
r="$(fresh_repo)"
doc "$r" reference/v0.0.1_operators.md "operators v1"
commit "$r" init
git -C "$r" tag v0.0.1
doc "$r" reference/v0.0.1_operators.md "operators v1 edited"
commit "$r" edit-frozen
run "$r" validate
expect "modifying a frozen (<=R) doc -> validate fails" 1 $?

# 4. Forward-only: with R=v0.0.1, adding a v0.0.2 copy and leaving frozen docs
#    untouched -> validate passes.
r="$(fresh_repo)"
doc "$r" reference/v0.0.1_operators.md "operators v1"
commit "$r" init
git -C "$r" tag v0.0.1
doc "$r" reference/v0.0.2_operators.md "operators v2"
commit "$r" add-inprogress
run "$r" validate
expect "adding a > R snapshot -> validate passes" 0 $?

# 5. Freeze: with R=v0.0.1, deleting a frozen doc -> validate fails.
r="$(fresh_repo)"
doc "$r" reference/v0.0.1_operators.md "operators v1"
commit "$r" init
git -C "$r" tag v0.0.1
git -C "$r" rm -q docs/reference/v0.0.1_operators.md
commit "$r" delete-frozen
run "$r" validate
expect "deleting a frozen (<=R) doc -> validate fails" 1 $?

# 6. Release consistency: R=v0.0.1, in-progress doc is v0.0.2.
r="$(fresh_repo)"
doc "$r" reference/v0.0.1_operators.md "operators v1"
commit "$r" init
git -C "$r" tag v0.0.1
doc "$r" reference/v0.0.2_operators.md "operators v2"
commit "$r" add-inprogress
run "$r" release v0.0.2
expect "release matching the in-progress version -> passes" 0 $?
run "$r" release v0.0.3
expect "release ahead of the in-progress docs -> fails" 1 $?
run "$r" release v0.0.1
expect "release not newer than R -> fails" 1 $?

echo "-----"
echo "$PASS passed, $FAIL failed"
[ "$FAIL" -eq 0 ]
