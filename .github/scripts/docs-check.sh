#!/usr/bin/env bash
# Docs versioning guard for LogosLang (pure bash, no Node/TypeScript).
#
# The website (ThobiasKnudsen/LogosLangWebsite) renders the files under docs/ using
# this model, so the guard lives here, next to the docs, and blocks bad doc edits at
# PR time. Files are named `vX.Y.Z_name.md`; the prefix is the version at which that
# page's content last changed. R = the newest `vX.Y.Z` git tag on this repo = the
# frozen line. Versions <= R are frozen; versions > R are the in-progress line.
#
#   validate            (run on every pull request)
#     - every file under docs/ carries a valid `vX.Y.Z_name.md` prefix;
#     - FREEZE: no snapshot whose version is <= R is added, modified, or deleted;
#     - FORWARD-ONLY: any changed snapshot must have a version > R.
#
#   release <version>   (run in release.yml when a `vX.Y.Z` tag is pushed)
#     - every in-progress snapshot (version > R) is named exactly <version>, so the
#       set about to be frozen is internally consistent, and <version> is newer than R.
#
# Exit 0 when clean, 1 on problems, 2 on usage error.

set -euo pipefail

DOCS_DIR="docs"
PROBLEMS=0

die_usage() {
	echo "usage: docs-check.sh <validate | release <version>>" >&2
	exit 2
}

# Echo the "X.Y.Z" of a `vX.Y.Z_name.md` path, or nothing if it does not match.
ver_of_file() {
	local base
	base="$(basename "$1")"
	if [[ "$base" =~ ^v([0-9]+)\.([0-9]+)\.([0-9]+)_.+\.md$ ]]; then
		printf '%s.%s.%s' "${BASH_REMATCH[1]}" "${BASH_REMATCH[2]}" "${BASH_REMATCH[3]}"
	fi
}

# Compare two "X.Y.Z" strings; echo -1 (a<b), 0 (a==b), or 1 (a>b).
ver_cmp() {
	if [ "$1" = "$2" ]; then echo 0; return; fi
	local highest
	highest="$(printf '%s\n%s\n' "$1" "$2" | sort -V | tail -n1)"
	if [ "$highest" = "$1" ]; then echo 1; else echo -1; fi
}

# The newest `vX.Y.Z` tag as a bare "X.Y.Z", or empty if none exist.
latest_release_tag() {
	# `|| true`: with no tags grep exits 1, which under `set -o pipefail` would abort
	# the script inside the `r="$(latest_release_tag)"` assignment.
	{
		git tag --list 'v*.*.*' \
			| grep -E '^v[0-9]+\.[0-9]+\.[0-9]+$' \
			| sed 's/^v//' \
			| sort -V \
			| tail -n1
	} || true
}

# Every doc must carry a valid prefix; unversioned files are silently dropped by the
# site model, so we fail loudly instead.
check_prefixes() {
	local file base
	while IFS= read -r file; do
		[ -z "$file" ] && continue
		base="$(basename "$file")"
		if [[ ! "$base" =~ ^v[0-9]+\.[0-9]+\.[0-9]+_.+\.md$ ]]; then
			echo "  - $file: missing or invalid version prefix (expected vX.Y.Z_name.md)"
			PROBLEMS=$((PROBLEMS + 1))
		fi
	done < <(find "$DOCS_DIR" -type f -name '*.md' 2>/dev/null | sort)
}

# Freeze + forward-only guard over the diff between the latest release (R) and HEAD.
check_diff() {
	local from="$1" r="$2" status file ver vstr
	while IFS=$'\t' read -r status file; do
		case "$file" in "$DOCS_DIR"/*) ;; *) continue ;; esac
		case "$status" in A | M | D) ;; *) continue ;; esac
		ver="$(ver_of_file "$file")"
		[ -z "$ver" ] && continue # reported by check_prefixes
		# Frozen when version <= R; the in-progress line (version > R) may change freely.
		[ "$(ver_cmp "$ver" "$r")" -gt 0 ] && continue
		vstr="v$ver (<= released v$r)"
		if [ "$status" = "D" ]; then
			echo "  - $file: deletes frozen doc ($vstr)"
		elif [ "$status" = "M" ]; then
			echo "  - $file: modifies frozen doc ($vstr); copy it to a newer version instead"
		else
			echo "  - $file: adds a doc to already-released version $vstr"
		fi
		PROBLEMS=$((PROBLEMS + 1))
	done < <(git diff --no-renames --name-status "$from" HEAD -- "$DOCS_DIR")
}

# At release time every in-progress snapshot (version > R) must be named exactly the
# version being released, so the frozen set is consistent.
check_release() {
	local target="$1" r="$2" file ver
	if [ -n "$r" ] && [ "$(ver_cmp "$target" "$r")" -le 0 ]; then
		echo "  - (release): v$target is not newer than the last released v$r"
		PROBLEMS=$((PROBLEMS + 1))
	fi
	while IFS= read -r file; do
		[ -z "$file" ] && continue
		ver="$(ver_of_file "$file")"
		[ -z "$ver" ] && continue # reported by check_prefixes
		# In progress when there is no release yet, or version > R.
		if [ -n "$r" ] && [ "$(ver_cmp "$ver" "$r")" -le 0 ]; then continue; fi
		if [ "$(ver_cmp "$ver" "$target")" -ne 0 ]; then
			echo "  - $file: in-progress doc is v$ver but the release target is v$target; rename it to match the version being released"
			PROBLEMS=$((PROBLEMS + 1))
		fi
	done < <(find "$DOCS_DIR" -type f -name '*.md' 2>/dev/null | sort)
}

report() {
	local heading="$1"
	if [ "$PROBLEMS" -eq 0 ]; then
		echo "OK $heading: no problems."
		exit 0
	fi
	echo "FAIL $heading: $PROBLEMS problem(s) (see above)." >&2
	exit 1
}

run_validate() {
	local r
	r="$(latest_release_tag)"
	check_prefixes
	if [ -n "$r" ]; then
		echo "(latest released version R = v$r)"
		check_diff "v$r" "$r"
	else
		echo "(no release tags yet - nothing is frozen)"
	fi
	report "docs validate"
}

run_release() {
	local target="${1#v}" r
	if [[ ! "$target" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
		echo "FAIL release: invalid version \"$1\" (expected vX.Y.Z)" >&2
		exit 2
	fi
	# R = the highest existing tag that is strictly older than the target being released.
	r=""
	while IFS= read -r t; do
		[ -z "$t" ] && continue
		[ "$t" = "$target" ] && continue
		[ "$(ver_cmp "$t" "$target")" -ge 0 ] && continue
		if [ -z "$r" ] || [ "$(ver_cmp "$t" "$r")" -gt 0 ]; then r="$t"; fi
	done < <(git tag --list 'v*.*.*' | grep -E '^v[0-9]+\.[0-9]+\.[0-9]+$' | sed 's/^v//')

	check_prefixes
	check_release "$target" "$r"
	echo "(releasing v$target; previous released R = ${r:+v}${r:-none})"
	report "docs release v$target"
}

main() {
	local mode="${1:-}"
	case "$mode" in
		validate) run_validate ;;
		release)
			[ -n "${2:-}" ] || die_usage
			run_release "$2"
			;;
		*) die_usage ;;
	esac
}

main "$@"
