#!/usr/bin/env bash
# Scaffold a new docs snapshot file with the correct version prefix, so authors don't
# have to hand-name `vX.Y.Z_name.md` and risk tripping the docs guard (docs-check.sh).
#
#   .github/scripts/new-doc.sh <dir/name> [version]
#
# Examples:
#   .github/scripts/new-doc.sh reference/operators           # picks the in-progress version
#   .github/scripts/new-doc.sh guides/internals/logic-graph  # nested folders are fine
#   .github/scripts/new-doc.sh getting-started/intro 0.1.0   # pin the version explicitly
#
# The default version is the current in-progress line (the newest prefix already past
# the last release R), or one patch above R if work has not started, or 0.0.1 for a
# fresh repo. You can always rename the file afterwards (e.g. for a minor/major bump);
# the guard checks the name is valid and, at release time, that every in-progress doc
# matches the version being released.
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

latest_release_tag() {
	{
		git tag --list 'v*.*.*' \
			| grep -E '^v[0-9]+\.[0-9]+\.[0-9]+$' \
			| sed 's/^v//' \
			| sort -V \
			| tail -n1
	} || true
}

# The newest in-progress version across the docs (highest prefix strictly > R), or empty.
in_progress_version() { # R
	local r="$1" best="" ver base
	while IFS= read -r file; do
		[ -z "$file" ] && continue
		base="$(basename "$file")"
		[[ "$base" =~ ^v([0-9]+)\.([0-9]+)\.([0-9]+)_.+\.md$ ]] || continue
		ver="${BASH_REMATCH[1]}.${BASH_REMATCH[2]}.${BASH_REMATCH[3]}"
		[ -n "$r" ] && [ "$(ver_cmp "$ver" "$r")" -le 0 ] && continue # frozen line
		if [ -z "$best" ] || [ "$(ver_cmp "$ver" "$best")" -gt 0 ]; then best="$ver"; fi
	done < <(find "$DOCS_DIR" -type f -name '*.md' 2>/dev/null)
	printf '%s' "$best"
}

prettify() { # kebab/snake -> Title Case
	echo "$1" | tr '_-' '  ' | awk '{ for (i=1;i<=NF;i++) $i=toupper(substr($i,1,1)) substr($i,2) }1'
}

main() {
	local target="${1:-}" want="${2:-}"
	[ -n "$target" ] || fail "usage: new-doc.sh <dir/name> [version]"

	# Split "<dir>/<name>", tolerate a trailing .md.
	target="${target%.md}"
	local dir name
	if [[ "$target" == */* ]]; then
		dir="${target%/*}"
		name="${target##*/}"
	else
		dir=""
		name="$target"
	fi
	[ -n "$name" ] || fail "could not read a doc name from \"$1\" (expected <dir/name>)."
	[[ "$name" =~ ^v[0-9]+\.[0-9]+\.[0-9]+_ ]] && fail "give the name without a version prefix; the version is added for you (got \"$name\")."

	local r version defaulted=0
	r="$(latest_release_tag)"

	if [ -n "$want" ]; then
		want="${want#v}"
		[[ "$want" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || fail "\"$2\" is not a version (expected X.Y.Z)."
		if [ -n "$r" ] && [ "$(ver_cmp "$want" "$r")" -le 0 ]; then
			fail "v$want is already released (<= v$r); new docs must target a newer version."
		fi
		version="$want"
	else
		defaulted=1
		version="$(in_progress_version "$r")"
		if [ -z "$version" ]; then
			if [ -n "$r" ]; then
				# one patch above the last release
				version="$(echo "$r" | awk -F. '{ printf "%s.%s.%s", $1, $2, $3 + 1 }')"
			else
				version="0.0.1"
			fi
		fi
	fi

	local file_name="v${version}_${name}.md"
	local rel_path
	if [ -n "$dir" ]; then rel_path="$DOCS_DIR/$dir/$file_name"; else rel_path="$DOCS_DIR/$file_name"; fi

	[[ "$file_name" =~ ^v[0-9]+\.[0-9]+\.[0-9]+_.+\.md$ ]] || fail "\"$file_name\" is not a valid snapshot filename."
	[ -e "$rel_path" ] && fail "$rel_path already exists; refusing to overwrite."

	local title
	title="$(prettify "$name")"
	mkdir -p "$(dirname "$rel_path")"
	printf -- '---\ntitle: %s\n---\n\n# %s\n\n' "$title" "$title" >"$rel_path"

	echo "OK created $rel_path"
	echo "   version v$version${r:+ (last released v$r)}${r:-  (no releases tagged yet)}"
	if [ "$defaulted" = 1 ]; then
		echo "   -> defaulted to the in-progress version. For a minor/major bump, rename the prefix"
		echo "      or re-run with an explicit version."
	fi
}

main "$@"
