#!/usr/bin/env bash
# Checks the showcase project's curated history.
#
# Replays the patch series in payments-history/ onto an empty tree and fails
# unless the result matches payments/ exactly. It also checks that every
# change declares its Areas in an `Areas:` trailer, that each of those areas
# is declared in payments/.whetstone/config.toml, and that the changes
# together cover every declared area.
#
# Usage: showcase/check-history.sh [--keep DIR]
#   --keep DIR  replay into the new directory DIR and keep it as a git
#               repository, for example to edit the history.
set -euo pipefail

showcase=$(cd "$(dirname "$0")" && pwd)
project="$showcase/payments"
patches="$showcase/payments-history"
config="$project/.whetstone/config.toml"

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

# Read the declared areas: every quoted string in the `areas = [...]` array.
declared=$(awk '/^areas *= *\[/ { grab = 1 } grab { print } grab && /\]/ { exit }' "$config" |
  grep -o '"[^"]*"' | tr -d '"')
[ -n "$declared" ] || fail "$config declares no areas"

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
    echo "$declared" | grep -Fqx -- "$area" ||
      fail "\"$subject\" declares area \"$area\", which $config does not declare"
    covered=$(printf '%s\n%s' "$covered" "$area")
  done
  printf '  %02d  %-55s %s\n' "$changes" "$subject" "$trailer"
done < <(git -C "$replay" rev-list --reverse HEAD)

while read -r area; do
  echo "$covered" | grep -Fqx -- "$area" || fail "no change covers the area \"$area\""
done <<<"$declared"

echo "ok: $changes changes reproduce showcase/payments exactly ($files files)"
echo "ok: every change declares its areas, and together they cover: $(echo "$declared" | paste -sd, - | sed 's/,/, /g')"
if [ "$replay" != "$work/replay" ]; then
  echo "kept the replayed repository in $replay"
fi
