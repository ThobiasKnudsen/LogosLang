#!/usr/bin/env bash
# Docs versioning guard for LogosLang (pure bash, no Node/TypeScript).
#
# Each version is a complete, self-contained tree under `docs/vX.Y.Z/`. To cut a new
# version you copy the previous version's folder and edit/add/remove/restructure freely
# within it. The website (ThobiasKnudsen/LogosLangWebsite) renders these folders, so the
# guard lives here, next to the docs, and blocks bad edits at PR time. R = the newest
# `vX.Y.Z` git tag on this repo = the frozen line; versions <= R are frozen.
#
#   validate            (run on every pull request)
#     - docs/ contains only `vX.Y.Z/` folders (no stray files), each with >= 1 page;
#     - FREEZE: no file under a released folder (version <= R) is added/modified/deleted.
#
#   release <version>   (run in release.yml when a `vX.Y.Z` tag is pushed)
#     - every in-progress version folder (> R) is named exactly `v<version>`, and
#       `<version>` is newer than R (so the folder about to be frozen is the release).
#
# Exit 0 when clean, 1 on problems, 2 on usage error.

set -euo pipefail

DOCS_DIR="docs"
PROBLEMS=0

die_usage() {
	echo "usage: docs-check.sh <validate | release <version>>" >&2
	exit 2
}

# Echo the "X.Y.Z" of a `vX.Y.Z` folder name, or nothing if it does not match.
ver_of_dir() {
	if [[ "$1" =~ ^v([0-9]+)\.([0-9]+)\.([0-9]+)$ ]]; then
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

# docs/ must contain only `vX.Y.Z/` folders, each holding at least one page.
check_layout() {
	local entry base ver
	for entry in "$DOCS_DIR"/*; do
		[ -e "$entry" ] || continue # empty docs/
		base="$(basename "$entry")"
		if [ ! -d "$entry" ]; then
			echo "  - $entry: stray file at docs root (docs/ holds only vX.Y.Z/ folders)"
			PROBLEMS=$((PROBLEMS + 1))
			continue
		fi
		ver="$(ver_of_dir "$base")"
		if [ -z "$ver" ]; then
			echo "  - $entry: not a version folder (expected docs/vX.Y.Z/)"
			PROBLEMS=$((PROBLEMS + 1))
			continue
		fi
		if [ -z "$(find "$entry" -type f -name '*.md' -print -quit 2>/dev/null)" ]; then
			echo "  - $entry: version folder has no .md pages"
			PROBLEMS=$((PROBLEMS + 1))
		fi
	done
}

# Freeze guard: nothing under a released folder (version <= R) may change.
check_freeze() {
	local r="$1" status file rest verdir ver
	while IFS=$'\t' read -r status file; do
		case "$file" in "$DOCS_DIR"/*) ;; *) continue ;; esac
		case "$status" in A | M | D) ;; *) continue ;; esac
		rest="${file#"$DOCS_DIR"/}"
		verdir="${rest%%/*}"
		ver="$(ver_of_dir "$verdir")"
		[ -z "$ver" ] && continue # layout problem, reported by check_layout
		if [ "$(ver_cmp "$ver" "$r")" -le 0 ]; then
			echo "  - $file: changes frozen version v$ver (<= released v$r); released versions are immutable - start a new version folder instead"
			PROBLEMS=$((PROBLEMS + 1))
		fi
	done < <(git diff --no-renames --name-status "v$r" HEAD -- "$DOCS_DIR")
}

# At release time every in-progress folder (> R) must be the version being released.
check_release() {
	local target="$1" r="$2" entry base ver
	if [ -n "$r" ] && [ "$(ver_cmp "$target" "$r")" -le 0 ]; then
		echo "  - (release): v$target is not newer than the last released v$r"
		PROBLEMS=$((PROBLEMS + 1))
	fi
	for entry in "$DOCS_DIR"/*/; do
		[ -d "$entry" ] || continue
		base="$(basename "$entry")"
		ver="$(ver_of_dir "$base")"
		[ -z "$ver" ] && continue # reported by check_layout
		# In progress when there is no release yet, or version > R.
		if [ -n "$r" ] && [ "$(ver_cmp "$ver" "$r")" -le 0 ]; then continue; fi
		if [ "$(ver_cmp "$ver" "$target")" -ne 0 ]; then
			echo "  - ${entry}: in-progress version v$ver but the release target is v$target; the folder being released must be v$target"
			PROBLEMS=$((PROBLEMS + 1))
		fi
	done
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
	check_layout
	if [ -n "$r" ]; then
		echo "(latest released version R = v$r)"
		check_freeze "$r"
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
	# R = the highest existing tag strictly older than the target being released.
	r=""
	while IFS= read -r t; do
		[ -z "$t" ] && continue
		[ "$t" = "$target" ] && continue
		[ "$(ver_cmp "$t" "$target")" -ge 0 ] && continue
		if [ -z "$r" ] || [ "$(ver_cmp "$t" "$r")" -gt 0 ]; then r="$t"; fi
	done < <(git tag --list 'v*.*.*' | grep -E '^v[0-9]+\.[0-9]+\.[0-9]+$' | sed 's/^v//')

	check_layout
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
