#!/usr/bin/env bash
# Tests for next-version.sh against throwaway git repositories.
set -euo pipefail

script="$(cd "$(dirname "$0")" && pwd)/next-version.sh"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
failures=0

repo() {
  rm -rf "$work/repo" && mkdir "$work/repo" && cd "$work/repo"
  git init -q -b main
  git config user.name test && git config user.email test@example.com
  git commit -q --allow-empty -m "chore: initial"
}
commit() { git commit -q --allow-empty -m "$1"; }
check() {
  local name="$1" expected="$2" actual
  actual="$("$script")"
  if [ "$actual" = "$expected" ]; then
    echo "ok - $name"
  else
    echo "not ok - $name: expected '$expected', got '$actual'"
    failures=$((failures + 1))
  fi
}

repo; commit "feat(escpos): add printers"
check "no tag: feat bumps minor from the fallback" "0.2.0"

repo; commit "Bump preact from 1 to 2"; commit "docs: readme"
check "no tag: nothing release-worthy" ""

repo; commit "feat: one"; git tag v0.2.0; commit "fix: two"
check "fix after a tag bumps patch" "0.2.1"

repo; commit "feat: one"; git tag v0.2.0; commit "Bump tokio (#3)"; commit "ci: tweak"
check "dependabot and ci commits after a tag don't release" ""

repo; commit "feat: one"; git tag v0.2.0; commit "fix: a"; commit "feat(api): b"
check "highest bump wins" "0.3.0"

repo; git tag v0.2.0; commit "feat: older minor"; commit "fix: newer patch"; commit "docs: newest"
check "an older, higher bump still wins" "0.3.0"

repo; git tag v0.2.0; commit "refactor!: drop star"
check "bang marks a breaking change" "1.0.0"

repo; git tag v1.4.2; commit $'fix: thing\n\nBREAKING CHANGE: config renamed'
check "BREAKING CHANGE footer" "2.0.0"

repo; git tag v0.2.0; commit "feat: a"; git tag v0.3.0; git tag v0.10.0 HEAD~0; commit "perf: b"
check "latest tag is chosen by version, not by name" "0.10.1"

repo; git tag v0.2.0; commit "feature: a"; commit "Merge branch x"
check "feature counts as minor" "0.3.0"

repo; git tag v0.2.0; commit "feat:no space is still conventional"
check "no space after colon" "0.3.0"

repo; git tag v0.2.0; commit "featuring nothing: not a type"
check "words starting with feat aren't feat" ""

exit $failures
