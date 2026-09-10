#!/usr/bin/env bash
# Developer Certificate of Origin guard for LogosLang (pure bash + git, no actions).
#
# Every commit a pull request adds must carry a `Signed-off-by:` line whose email is the
# commit author's, which is what `git commit -s` writes. By adding it the author
# certifies the DCO 1.1 (reproduced verbatim in ./DCO at the repository root): that they
# wrote the change, or may submit somebody else's work under this project's license. It
# is the lightweight alternative to a contributor agreement, and it is the record that
# every line here was given deliberately by someone entitled to give it.
#
#   dco-check.sh <base> <head>
#
# Checks the commits in `<base>..<head>`, which in CI are the pull request's base and
# head shas. Merge commits are skipped: GitHub generates them and nobody authors them.
#
# Exit 0 when every checked commit is signed off, 1 when one is not, 2 on usage error.

set -euo pipefail

# Commits authored before this date predate the rule. It was added on 10 September 2026
# to a repository with 268 commits of sole-authored history behind it, 106 of them
# already on `dev` and bound for the next pull request into `main`. Failing those would
# say nothing about provenance (they are all the copyright holder's own) and would block
# every merge until history was rewritten, which would cost more than it proves. The
# date is the day after the rule landed, so the work already done on the day it landed
# is exempt too. Drop this clause once no unsigned commit can still reach a pull
# request.
DCO_EFFECTIVE="2026-09-11"

if [ "$#" -ne 2 ]; then
	echo "usage: dco-check.sh <base> <head>" >&2
	exit 2
fi

BASE="$1"
HEAD="$2"

if ! git rev-parse --verify --quiet "$BASE^{commit}" >/dev/null; then
	echo "::error::dco: base '$BASE' is not a commit in this repository. A shallow clone cannot see it; check out with fetch-depth: 0." >&2
	exit 2
fi
if ! git rev-parse --verify --quiet "$HEAD^{commit}" >/dev/null; then
	echo "::error::dco: head '$HEAD' is not a commit in this repository." >&2
	exit 2
fi

CHECKED=0
EXEMPT=0
PROBLEMS=0

# Lowercase everything before comparing: git preserves the case an author typed, and
# `Thobias@Example.com` and `thobias@example.com` are the same mailbox.
lower() { printf '%s' "$1" | tr '[:upper:]' '[:lower:]'; }

while IFS= read -r sha; do
	[ -n "$sha" ] || continue

	authored="$(git log -1 --format='%ad' --date=short "$sha")"
	if [[ "$authored" < "$DCO_EFFECTIVE" ]]; then
		EXEMPT=$((EXEMPT + 1))
		continue
	fi

	CHECKED=$((CHECKED + 1))
	author_email="$(lower "$(git log -1 --format='%ae' "$sha")")"
	subject="$(git log -1 --format='%s' "$sha")"

	# Every address signed off on this commit, one per line. `|| true` because grep
	# exits 1 on no match, which under `set -e` would abort the run instead of
	# reporting the commit as unsigned.
	signed_by="$(
		git log -1 --format='%B' "$sha" \
			| grep -iE '^[[:space:]]*Signed-off-by:[[:space:]]*.*<[^>]+>[[:space:]]*$' \
			| sed -E 's/.*<([^>]+)>.*/\1/' \
			| tr '[:upper:]' '[:lower:]' \
			|| true
	)"

	if printf '%s\n' "$signed_by" | grep -qxF "$author_email"; then
		continue
	fi

	PROBLEMS=$((PROBLEMS + 1))
	if [ -z "$signed_by" ]; then
		echo "::error::dco: ${sha:0:8} \"$subject\" has no Signed-off-by line (author: $author_email)."
	else
		echo "::error::dco: ${sha:0:8} \"$subject\" is signed off by $(printf '%s' "$signed_by" | tr '\n' ' ')but its author is $author_email."
	fi
done < <(git rev-list --no-merges "$BASE..$HEAD")

if [ "$PROBLEMS" -gt 0 ]; then
	cat >&2 <<-MSG

		$PROBLEMS of $CHECKED commit(s) are not signed off.

		Sign off means one line at the end of the commit message:

		    Signed-off-by: Your Name <your.email@example.com>

		Add it with \`git commit -s\`. To fix what is already committed:

		    last commit:   git commit --amend -s --no-edit
		    several:       git rebase --signoff $BASE

		then force-push the branch. What the line certifies is in ./DCO, and
		CONTRIBUTING.md explains why this project asks for it.
	MSG
	exit 1
fi

echo "OK: $CHECKED commit(s) signed off${EXEMPT:+, $EXEMPT exempt (authored before $DCO_EFFECTIVE)}."
