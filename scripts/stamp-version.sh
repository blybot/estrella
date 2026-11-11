#!/usr/bin/env bash
# Set the crate version in Cargo.toml and Cargo.lock (CI only; not committed).
# Usage: scripts/stamp-version.sh 1.2.3
set -euo pipefail

version="${1:?usage: $0 <version>}"
root="$(cd "$(dirname "$0")/.." && pwd)"

sed -i -E "0,/^version = \".*\"$/s//version = \"$version\"/" "$root/Cargo.toml"
sed -i -E "/^name = \"estrella\"$/{n;s/^version = \".*\"$/version = \"$version\"/}" "$root/Cargo.lock"

grep -qx "version = \"$version\"" "$root/Cargo.toml"
grep -A1 -x 'name = "estrella"' "$root/Cargo.lock" | grep -qx "version = \"$version\""
echo "Stamped version $version"
