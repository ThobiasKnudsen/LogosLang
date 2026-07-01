#!/usr/bin/env bash
# Self-test for docs-check.sh (folder-per-version model). Builds throwaway git repos
# with docs/vX.Y.Z/ folders + tags and asserts the guard's exit code. Pure bash + git.
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

fresh_repo() {
	local dir
	dir="$(mktemp -d)"
	TMPDIRS+=("$dir")
	git -C "$dir" init -q
	git -C "$dir" config user.email t@example.com
	git -C "$dir" config user.name tester
	git -C "$dir" config commit.gpgsign false
	printf '%s' "$dir"
}

doc() { # repo, path-under-docs (incl vX.Y.Z/...), body
	local full="$1/docs/$2"
	mkdir -p "$(dirname "$full")"
	printf '# %s\n' "$3" >"$full"
}

commit() { git -C "$1" add -A >/dev/null; git -C "$1" commit -qm "$2"; }

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

# 1. No tags, valid layout -> validate passes.
r="$(fresh_repo)"
doc "$r" v0.0.1/reference/operators.md "operators v1"
commit "$r" init
run "$r" validate
expect "no tags + valid version folders -> validate passes" 0 $?

# 2. A stray file at the docs root -> validate fails.
r="$(fresh_repo)"
doc "$r" v0.0.1/reference/operators.md "operators v1"
printf 'stray\n' >"$r/docs/README.md"
commit "$r" init
run "$r" validate
expect "stray file at docs root -> validate fails" 1 $?

# 3. A non-version top-level folder -> validate fails.
r="$(fresh_repo)"
doc "$r" v0.0.1/reference/operators.md "operators v1"
doc "$r" latest/reference/operators.md "operators"
commit "$r" init
run "$r" validate
expect "non-version folder at docs root -> validate fails" 1 $?

# 4. An empty version folder (no .md) -> validate fails.
r="$(fresh_repo)"
doc "$r" v0.0.1/reference/operators.md "operators v1"
mkdir -p "$r/docs/v0.0.2/reference"
printf 'notes\n' >"$r/docs/v0.0.2/reference/notes.txt" # not a .md
commit "$r" init
run "$r" validate
expect "version folder with no .md pages -> validate fails" 1 $?

# 5. Freeze: with R=v0.0.1, editing a file in the released folder -> validate fails.
r="$(fresh_repo)"
doc "$r" v0.0.1/reference/operators.md "operators v1"
commit "$r" init
git -C "$r" tag v0.0.1
doc "$r" v0.0.1/reference/operators.md "operators v1 edited"
commit "$r" edit-frozen
run "$r" validate
expect "editing a frozen (<=R) version folder -> validate fails" 1 $?

# 6. Forward-only: with R=v0.0.1, adding a new v0.0.2 folder (v0.0.1 untouched) passes.
r="$(fresh_repo)"
doc "$r" v0.0.1/reference/operators.md "operators v1"
commit "$r" init
git -C "$r" tag v0.0.1
doc "$r" v0.0.2/reference/operators.md "operators v2"
doc "$r" v0.0.2/guides/internals/logic-graph.md "logic graph"
commit "$r" add-next-version
run "$r" validate
expect "adding a new v>R version folder -> validate passes" 0 $?

# 7. Freeze: with R=v0.0.1, deleting a file from the released folder -> validate fails.
r="$(fresh_repo)"
doc "$r" v0.0.1/reference/operators.md "operators v1"
doc "$r" v0.0.1/guides/intro.md "intro"
commit "$r" init
git -C "$r" tag v0.0.1
git -C "$r" rm -q docs/v0.0.1/guides/intro.md
commit "$r" delete-frozen
run "$r" validate
expect "deleting a file from a frozen version folder -> validate fails" 1 $?

# 8. Release consistency: R=v0.0.1, in-progress folder is v0.0.2.
r="$(fresh_repo)"
doc "$r" v0.0.1/reference/operators.md "operators v1"
commit "$r" init
git -C "$r" tag v0.0.1
doc "$r" v0.0.2/reference/operators.md "operators v2"
commit "$r" add-next-version
run "$r" release v0.0.2
expect "release matching the in-progress folder -> passes" 0 $?
run "$r" release v0.0.3
expect "release ahead of the in-progress folder -> fails" 1 $?
run "$r" release v0.0.1
expect "release not newer than R -> fails" 1 $?

echo "-----"
echo "$PASS passed, $FAIL failed"
[ "$FAIL" -eq 0 ]
