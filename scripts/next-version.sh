#!/usr/bin/env bash
# Print the next release version (X.Y.Z) from conventional commits since the
# latest vX.Y.Z tag reachable from HEAD, or print nothing if none of them
# warrants a release.
#
#   feat, feature                       → minor
#   fix, bugfix, perf, refactor         → patch
#   type! or a BREAKING CHANGE: footer  → major
#   anything else (docs, ci, chore, dependabot's "Bump x", ...) → no release
#
# With no tag yet, all history is considered against FALLBACK_VERSION (the
# last upstream release).
set -euo pipefail

fallback="${FALLBACK_VERSION:-0.1.2}"
latest="$(git tag --merged HEAD --list 'v[0-9]*.[0-9]*.[0-9]*' --sort=-v:refname | head -1)"
if [ -n "$latest" ]; then
  base="${latest#v}"
  range="$latest..HEAD"
else
  base="$fallback"
  range="HEAD"
fi

breaking='^[a-zA-Z]+(\([^)]*\))?!:'
minor='^(feat|feature)(\([^)]*\))?:'
patch='^(fix|bugfix|perf|refactor)(\([^)]*\))?:'

bump=0
while IFS= read -r -d '' message; do
  subject="${message%%$'\n'*}"
  if [[ $subject =~ $breaking ]] || grep -qE '^BREAKING[ -]CHANGE:' <<< "$message"; then
    level=3
  elif [[ $subject =~ $minor ]]; then
    level=2
  elif [[ $subject =~ $patch ]]; then
    level=1
  else
    level=0
  fi
  if (( level > bump )); then bump=$level; fi
done < <(git log -z --no-merges --format='%B' "$range")

IFS=. read -r major minor_v patch_v <<< "$base"
case $bump in
  3) echo "$((major + 1)).0.0" ;;
  2) echo "$major.$((minor_v + 1)).0" ;;
  1) echo "$major.$minor_v.$((patch_v + 1))" ;;
esac
