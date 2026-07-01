#!/usr/bin/env bash
# Start a new docs version by copying the latest version's folder, so the new version
# begins from the previous one and you edit/add/remove/restructure freely within it.
#
#   .github/scripts/new-version.sh [version]
#
# Examples:
#   .github/scripts/new-version.sh          # patch-bump the highest version folder
#   .github/scripts/new-version.sh 0.1.0    # pin the new version explicitly
#
# The new version must be newer than every existing version folder. After copying,
# add a page by creating a file in the new folder, remove one by deleting its file,
# and restructure by moving files around - all within the new version's folder only.
# The docs guard (docs-check.sh) checks the layout and, at release time, that the
# in-progress folder matches the version being released.
#
# Exit 0 on success, 1 on a bad request.

set -euo pipefail

DOCS_DIR="docs"

fail() {
	echo "FAIL $1" >&2
	exit 1
}

ver_cmp() { # echo -1 (a<b), 0 (a==b), 1 (a>b)
	if [ "$1" = "$2" ]; then echo 0; return; fi
	local highest
	highest="$(printf '%s\n%s\n' "$1" "$2" | sort -V | tail -n1)"
	if [ "$highest" = "$1" ]; then echo 1; else echo -1; fi
}

# The highest existing `vX.Y.Z` folder as a bare "X.Y.Z", or empty if none.
highest_version() {
	{
		find "$DOCS_DIR" -mindepth 1 -maxdepth 1 -type d -name 'v*.*.*' 2>/dev/null \
			| sed 's#.*/v##' \
			| grep -E '^[0-9]+\.[0-9]+\.[0-9]+$' \
			| sort -V \
			| tail -n1
	} || true
}

main() {
	local want="${1:-}" highest version
	highest="$(highest_version)"

	if [ -n "$want" ]; then
		want="${want#v}"
		[[ "$want" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || fail "\"$1\" is not a version (expected X.Y.Z)."
		version="$want"
	else
		if [ -n "$highest" ]; then
			version="$(echo "$highest" | awk -F. '{ printf "%s.%s.%s", $1, $2, $3 + 1 }')"
		else
			version="0.0.1"
		fi
	fi

	if [ -n "$highest" ] && [ "$(ver_cmp "$version" "$highest")" -le 0 ]; then
		fail "v$version is not newer than the highest existing folder v$highest."
	fi

	local dest="$DOCS_DIR/v$version"
	[ -e "$dest" ] && fail "$dest already exists; refusing to overwrite."

	if [ -n "$highest" ]; then
		cp -r "$DOCS_DIR/v$highest" "$dest"
		echo "OK created $dest (copied from v$highest)"
		echo "   edit/add/remove/restructure pages within $dest, then commit."
	else
		mkdir -p "$dest/getting-started"
		printf -- '---\ntitle: Introduction\n---\n\n# Introduction\n\n' >"$dest/getting-started/introduction.md"
		echo "OK created $dest with a starter page (no previous version to copy)."
	fi
}

main "$@"
