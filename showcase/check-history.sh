#!/usr/bin/env bash
# Checks the showcase project's curated history.
#
# Replays the patch series in payments-history/ onto an empty tree and fails
# unless the result matches payments/ exactly. It also checks that every
# change names its areas in an `Areas:` trailer, that each of those is the ID
# of one of Whetstone's default areas (the file names, without `.md`, in
# crates/whetstone-core/areas/), and that the changes together cover every
# default area.
#
# Usage: showcase/check-history.sh [--keep DIR]
#   --keep DIR  replay into the new directory DIR and keep it as a git
#               repository, for example to edit the history.
set -euo pipefail

showcase=$(cd "$(dirname "$0")" && pwd)
project="$showcase/payments"
patches="$showcase/payments-history"
defaults="$showcase/../crates/whetstone-core/areas"

fail() {
  echo "check-history: $*" >&2
  exit 1
}

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
replay="$work/replay"
if [ "$#" -gt 0 ]; then
  [ "$1" = "--keep" ] && [ "$#" -eq 2 ] || fail "usage: $0 [--keep DIR]"
  [ ! -e "$2" ] || fail "$2 already exists"
  replay="$2"
fi

# Apply the series to an empty repository. CI has no git identity, so lend
# `git am` one when none is configured.
identity=()
if ! git config user.email >/dev/null; then
  identity=(-c user.name="Showcase replay" -c user.email="showcase-replay@localhost")
fi
git -c init.defaultBranch=main init -q "$replay"
git ${identity[@]+"${identity[@]}"} -C "$replay" am -q --committer-date-is-author-date \
  "$patches"/*.patch

# Compare the replayed tree with the project's files as git sees them:
# tracked files plus untracked ones that are not ignored, so build output
# such as target/ is left out.
expected="$work/expected"
actual="$work/actual"
mkdir "$expected" "$actual"
(cd "$project" && git ls-files -z --cached --others --exclude-standard -- . |
  tar -c -f - --null -T -) | tar -x -f - -C "$expected"
git -C "$replay" archive HEAD | tar -x -f - -C "$actual"
if ! git diff --no-index --exit-code "$expected" "$actual"; then
  fail "the patch series does not reproduce showcase/payments (diff above: a/ is the project, b/ the replay)"
fi
files=$(git -C "$replay" ls-files | wc -l | tr -d ' ')

# The default area IDs: the default area files' names without `.md`, in
# file-name order.
ids=""
for file in "$defaults"/*.md; do
  [ -e "$file" ] || fail "$defaults has no area files"
  ids=$(printf '%s\n%s' "$ids" "$(basename "$file" .md)")
done
ids=${ids#$'\n'}

covered=""
changes=0
while read -r sha; do
  changes=$((changes + 1))
  subject=$(git -C "$replay" log -1 --format=%s "$sha")
  trailer=$(git -C "$replay" log -1 --format='%(trailers:key=Areas,valueonly,separator=%x2C)' "$sha")
  [ -n "$trailer" ] || fail "\"$subject\" has no Areas: trailer"
  IFS=',' read -r -a areas <<<"$trailer"
  for area in "${areas[@]}"; do
    area=$(echo "$area" | sed 's/^ *//; s/ *$//')
    echo "$ids" | grep -Fqx -- "$area" ||
      fail "\"$subject\" names the area \"$area\", which is not the ID of a default area in $defaults"
    covered=$(printf '%s\n%s' "$covered" "$area")
  done
  printf '  %02d  %-55s %s\n' "$changes" "$subject" "$trailer"
done < <(git -C "$replay" rev-list --reverse HEAD)

while read -r area; do
  echo "$covered" | grep -Fqx -- "$area" || fail "no change covers the area \"$area\""
done <<<"$ids"

echo "ok: $changes changes reproduce showcase/payments exactly ($files files)"
echo "ok: every change names default area IDs, and together they cover: $(echo "$ids" | paste -sd, - | sed 's/,/, /g')"
if [ "$replay" != "$work/replay" ]; then
  echo "kept the replayed repository in $replay"
fi
