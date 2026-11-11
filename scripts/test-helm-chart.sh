#!/usr/bin/env bash
# Lint and render the Helm chart the ways a deployment can use it, and check
# that bad values are rejected. Run from anywhere; needs helm.
set -euo pipefail

chart="$(cd "$(dirname "$0")/../helm/estrella" && pwd)"
example="$chart/ci/example-values.yaml"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

pass() { echo "ok - $1"; }
fail() { echo "not ok - $1" >&2; exit 1; }

expect_render() {
  local name="$1"; shift
  helm template estrella "$chart" "$@" > "$work/out.yaml" || fail "$name"
  pass "$name"
}

expect_reject() {
  local name="$1" message="$2"; shift 2
  if helm template estrella "$chart" "$@" > /dev/null 2> "$work/err"; then
    fail "$name (rendered, expected an error)"
  fi
  grep -q -- "$message" "$work/err" || { cat "$work/err" >&2; fail "$name (wrong error)"; }
  pass "$name"
}

helm lint --strict "$chart" -f "$example" > /dev/null || fail "helm lint"
pass "helm lint"

expect_render "standalone with example values" -f "$example"
grep -q 'kind: Ingress' "$work/out.yaml" || fail "ingress rendered"
grep -q 'secretName: estrella-tls' "$work/out.yaml" || fail "tls secret defaulted"
grep -A1 'name: ESTRELLA_ADDRESS' "$work/out.yaml" | grep -q 'value: "192.168.1.50"' || fail "printer address env"
pass "ingress, tls and printer env rendered"

expect_render "ingress disabled" -f "$example" --set ingress.enabled=false
! grep -q 'kind: Ingress' "$work/out.yaml" || fail "ingress disabled renders no ingress"

expect_render "tls disabled" -f "$example" --set ingress.tls.enabled=false
! grep -q 'tls:' "$work/out.yaml" || fail "tls disabled renders no tls block"

expect_render "extraEnv overrides come last" -f "$example" \
  --set 'extraEnv[0].name=ESTRELLA_LISTEN' --set 'extraEnv[0].value=0.0.0.0:9000'
[ "$(grep -A1 'name: ESTRELLA_LISTEN' "$work/out.yaml" | grep -c value)" = 2 ] || fail "extraEnv rendered"

# As a dependency of a wrapper chart, with values nested under the chart name.
mkdir -p "$work/wrapper/charts"
cp -r "$chart" "$work/wrapper/charts/estrella"
cat > "$work/wrapper/Chart.yaml" <<YAML
apiVersion: v2
name: wrapper
version: 0.0.1
dependencies:
  - name: estrella
    version: 0.0.0
YAML
{ echo "global: {}"; echo "estrella:"; sed 's/^/  /' "$example"; } > "$work/wrapper/values.yaml"
helm template wrapper "$work/wrapper" > /dev/null || fail "wrapper chart"
pass "as a dependency of a wrapper chart"

expect_reject "printer is required" "printer" --set ingress.host=x
expect_reject "ingress host is required" "ingress.host is required" \
  --set printer.protocol=escpos --set printer.address=x
expect_reject "unknown protocol" "printer.protocol" -f "$example" --set printer.protocol=zpl
expect_reject "latest tag" "image.tag" -f "$example" --set image.tag=latest
expect_reject "unknown value" "additional properties" -f "$example" --set printr.address=x
