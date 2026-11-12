#!/usr/bin/env bash
# Run a built image the way the Helm chart does (read-only root, non-root,
# ESC/POS over TCP, example templates mounted read-only) and check the web UI,
# health endpoint, templates list and JSON API print path against a fake
# network printer.
# Usage: scripts/smoke-test-image.sh <image>   (DOCKER=nerdctl to override)
set -euo pipefail

image="${1:?usage: $0 <image>}"
repo="$(cd "$(dirname "$0")/.." && pwd)"
docker="${DOCKER:-docker}"
http_port="${HTTP_PORT:-18080}"
printer_port="${PRINTER_PORT:-19100}"
work="$(mktemp -d)"
name="estrella-smoke-$$"

cleanup() {
  "$docker" rm -f "$name" > /dev/null 2>&1 || true
  if [ -n "${sink_pid:-}" ]; then kill "$sink_pid" 2> /dev/null || true; fi
  rm -rf "$work"
}
trap cleanup EXIT

fail() {
  echo "not ok - $1" >&2
  "$docker" logs "$name" >&2 || true
  exit 1
}

# Fake printer: accept one connection and save what it receives.
python3 - "$printer_port" "$work/received.bin" <<'PY' &
import socket, sys
port, path = int(sys.argv[1]), sys.argv[2]
s = socket.socket()
s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
s.bind(("0.0.0.0", port))
s.listen(1)
conn, _ = s.accept()
with open(path, "wb") as f:
    while chunk := conn.recv(65536):
        f.write(chunk)
PY
sink_pid=$!

"$docker" run -d --name "$name" \
  --read-only --user 65532:65532 --cap-drop ALL \
  --add-host printer:host-gateway \
  -p "127.0.0.1:$http_port:8080" \
  -e ESTRELLA_PROTOCOL=escpos \
  -e "ESTRELLA_ADDRESS=printer:$printer_port" \
  -v "$repo/examples/templates:/templates:ro" \
  -e ESTRELLA_TEMPLATES_DIR=/templates \
  "$image" > /dev/null

for _ in $(seq 1 30); do
  curl -fsS "http://127.0.0.1:$http_port/healthz" > /dev/null 2>&1 && break
  sleep 1
done
[ "$(curl -fsS "http://127.0.0.1:$http_port/healthz")" = ok ] || fail "/healthz"
echo "ok - /healthz"

curl -fsS "http://127.0.0.1:$http_port/" | grep -qi '<div id="app"\|<!doctype html' || fail "web UI at /"
echo "ok - web UI served at /"

curl -fsS "http://127.0.0.1:$http_port/api/templates" | grep -q '"id":"notice"' || fail "templates listed"
echo "ok - templates listed"

response="$(curl -fsS -X POST "http://127.0.0.1:$http_port/api/json/print" \
  -H 'content-type: application/json' \
  -d '{"document":[{"type":"text","content":"smoke test"},{"type":"qr_code","data":"https://example.com"}],"cut":true}')"
echo "$response" | grep -q '"success": true' || fail "JSON API print: $response"
echo "ok - JSON API print"

for _ in $(seq 1 10); do
  kill -0 "$sink_pid" 2> /dev/null || break
  sleep 1
done
head -c 4 "$work/received.bin" | od -An -tx1 | tr -d ' \n' | grep -qx '1b401d50' || fail "printer received ESC/POS init"
grep -q 'smoke test' "$work/received.bin" || fail "printer received the text"
echo "ok - printer received $(wc -c < "$work/received.bin") bytes of ESC/POS"
