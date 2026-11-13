# Releasing estrella

Releases publish a multi-arch container image and a Helm chart to GitHub Packages (ghcr.io), then tag the commit and create a GitHub Release.

## Releasing a new version

Releases are automatic. Every push to `main` runs the checks (`.github/workflows/release.yml`), then `scripts/next-version.sh` reads the [conventional commits](https://www.conventionalcommits.org/) since the latest `vX.Y.Z` tag:

| Commits since the last release | Release |
|--------------------------------|---------|
| `type!:` or a `BREAKING CHANGE:` footer | major |
| `feat:` / `feature:` | minor |
| `fix:`, `bugfix:`, `perf:`, `refactor:` | patch |
| anything else (`docs:`, `ci:`, `chore:`, `build:`, dependabot's "Bump x") | none |

So merging dependency bumps on their own doesn't release; they ship with the next `feat`/`fix`. With no tags yet, the whole history is compared against upstream's last release, v0.1.2.

When a release is due, CI:
1. Stamps the version into `Cargo.toml` and `Cargo.lock` for the build (`scripts/stamp-version.sh`; not committed, so the committed version stays at the last manual bump)
2. Builds and pushes `ghcr.io/<owner>/estrella:X.Y.Z` (and `:X.Y`) for linux/amd64 and linux/arm64, on native runners for each
3. Packages the Helm chart as version `X.Y.Z` (appVersion `X.Y.Z`) and pushes it to `oci://ghcr.io/<owner>/helm/estrella`
4. Only once all of that succeeds, tags the commit `vX.Y.Z` and creates a GitHub Release

To re-run the release decision by hand (e.g. after a failed run): `gh workflow run release.yml`.

Pull requests run the same checks without publishing.

New packages on ghcr.io are private by default. Make `estrella` and `helm/estrella` public under the owner's Packages settings, or give your cluster pull credentials.

## What the CI does

| Job | Runner | What it does |
|-----|--------|--------------|
| `helm` | ubuntu-24.04 | `scripts/test-helm-chart.sh` and `scripts/test-next-version.sh` |
| `image` (check) | ubuntu-24.04 | Builds the image (which compiles the Rust binary and frontend) and runs `scripts/smoke-test-image.sh` against a fake TCP printer |
| `version` | ubuntu-24.04 | Decides the next version, or that there's nothing to release |
| `image` (release) | ubuntu-24.04 / ubuntu-24.04-arm | Builds and pushes each architecture by digest |
| `image-manifest` | ubuntu-24.04 | Combines the digests into the multi-arch `X.Y.Z` and `X.Y` tags |
| `chart` | ubuntu-24.04 | `helm package` + `helm push` to ghcr.io |
| `release` | ubuntu-24.04 | Tags `vX.Y.Z` and creates the GitHub Release |

CI doesn't run the Rust unit and golden tests, clippy, rustfmt or the Playwright e2e tests. Run them locally before pushing:

```bash
(cd frontend && npm ci && npm run build)   # embedded in the binary, so build it first
cargo fmt --check
cargo clippy -- -D warnings
cargo test
(cd frontend && npx playwright test)       # starts a server on :8090 unless one is running
```

Without libheif installed, add `--no-default-features` to the cargo commands, and start the e2e server yourself before running Playwright (it reuses a running server):

```bash
ESTRELLA_TEMPLATES_DIR=examples/templates cargo run --no-default-features -- serve --listen 0.0.0.0:8090
```

With Nix, `make test lint format-check test-e2e` runs the same checks inside `nix develop`.

## .deb packages

CI doesn't build `.deb` packages or the apt repository; those relied on Nix builds. To build an arm64 `.deb` by hand:

```bash
nix build .#static -o result
VERSION=X.Y.Z nfpm package --packager deb --target estrella_X.Y.Z_arm64.deb
```

The binary is built with `--no-default-features`, which disables HEIC/HEIF image support (requires the libheif C library). The container image links Debian's libheif and supports HEIC.
