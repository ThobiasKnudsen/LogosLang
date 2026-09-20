#!/usr/bin/env bash
# Fails on ruling history in Rust comments: dates, "ruled"/"superseded"/"amended",
# issue numbers outside a "stand-in for #N", DESIGN quotes. CLAUDE.md, "Comment rules".
set -uo pipefail
cd "$(dirname "$0")/../.."
lines=$(grep -rn --include='*.rs' -E '^\s*//' src tests)
months='January|February|March|April|May|June|July|August|September|October|November|December'
hits=$(
  { echo "$lines" | grep -E "\b20[0-9]{2}-[0-9]{2}-[0-9]{2}\b|\b($months) 20[0-9]{2}\b|\b[0-9]{1,2} ($months)\b" | grep -v Copyright | sed 's/^/date: /'
    echo "$lines" | grep -iE '\b(ruled|superseded|amended)\b|recorded as (rejected|declined)' | sed 's/^/ruling word: /'
    echo "$lines" | grep -E '#[0-9]+' | grep -vE 'stand-in for #[0-9]+' | sed 's/^/issue number: /'
    echo "$lines" | grep -E 'DESIGN[^"]*"[^"]+"' | sed 's/^/DESIGN quote: /'
  } | grep -v '^$' || true
)
if [ -n "$hits" ]; then
  echo "$hits"
  echo "comment-check: $(echo "$hits" | wc -l) comment line(s) carry ruling history; see CLAUDE.md 'Comment rules'"
  exit 1
fi
echo "comment-check: OK"
