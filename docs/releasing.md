# Releasing estrella

This document covers how releases are cut and published: a multi-arch container image and Helm chart on GitHub Packages (ghcr.io), and `.deb` packages via a flat apt repository hosted on GitHub Releases.

The apt repository is optional: without the GPG secrets below, releases still publish the image, chart and `.deb`, and the apt step is skipped with a notice.

## One-time setup

### 1. Generate a GPG signing key

This key signs the apt repository metadata so users can verify packages are authentic.

The keyring is stored in `.gnupg/` at the project root (gitignored). When you enter `nix develop`, `GNUPGHOME` is automatically set to this directory, so keys are portable across machines — just copy the `.gnupg/` folder.

```bash
# Enter the dev shell (sets GNUPGHOME=.gnupg/)
nix develop

# Generate a new key (RSA 4096, no passphrase for CI use)
gpg --full-generate-key
# Select: (1) RSA and RSA
# Key size: 4096
# Expiration: 0 (does not expire), or set a reasonable expiration
# Name: estrella release signing
# Email: your email
# Passphrase: leave empty (press Enter twice)

# Find your key ID
gpg --list-keys --keyid-format long
# Look for the line like: pub   rsa4096/ABCDEF1234567890
# The part after the slash is your KEY_ID
```

### 2. Add GitHub secrets

Export the private key and add it as a repository secret:

```bash
# Export private key (armor format)
gpg --armor --export-secret-keys YOUR_KEY_ID
```

Go to your GitHub repo → Settings → Secrets and variables → Actions, and add:

| Secret | Value |
|--------|-------|
| `GPG_PRIVATE_KEY` | The full armor-encoded private key output (including `-----BEGIN PGP PRIVATE KEY BLOCK-----` and `-----END PGP PRIVATE KEY BLOCK-----`) |
| `GPG_KEY_ID` | Your key ID (e.g., `ABCDEF1234567890`) |

That's it — no GitHub Pages setup needed. The `apt` release is created automatically by the first CI run.

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
4. Builds the static arm64 `.deb`
5. Only once all of that succeeds, tags the commit `vX.Y.Z` and creates a GitHub Release with the `.deb` attached
6. Updates the flat apt repository on the `apt` release, if the GPG secrets are set

Pull requests run the same checks (Rust, frontend e2e, Helm chart, container image smoke test) without publishing.

New packages on ghcr.io are private by default. Make `estrella` and `helm/estrella` public under the owner's Packages settings, or give your cluster pull credentials.

## How the apt repository works

The apt repo is hosted entirely on GitHub Releases — no gh-pages branch, no git history bloat. It uses two types of releases:

### Two releases per version

| Release | URL pattern | Contents |
|---------|------------|----------|
| **v0.1.1** (version tag) | `releases/download/v0.1.1/estrella_0.1.1_arm64.deb` | The `.deb` for this version + release notes. A normal release page for humans. |
| **apt** (permanent tag) | `releases/download/apt/Packages`, `releases/download/apt/estrella_*.deb`, etc. | ALL `.deb` files from every version, plus the apt metadata (`Packages`, `Release`, `InRelease`, `gpg.key`). This is what users point their `sources.list` at. |

The `apt` release is a long-lived "bucket" that accumulates every `.deb` ever published. Users configure their `sources.list` once and it never changes:

```
deb [arch=arm64 signed-by=...] https://github.com/eljojo/estrella/releases/download/apt ./
```

### Why a dedicated `apt` release?

The `Packages` file must list **all** available versions so users can install or pin any of them. If we put it on the version release (e.g., v0.1.1), it would only know about that one version. And `releases/latest/download/` only points to the most recent release, so old versions would disappear.

The dedicated `apt` release gives us a single stable URL that always has the complete package index.

### What's on the `apt` release

All files at `https://github.com/eljojo/estrella/releases/download/apt/`:

```
gpg.key                          # public GPG signing key
Packages                         # package index (lists all versions)
Packages.gz                      # compressed index
Release                          # repo metadata with checksums
Release.gpg                      # detached GPG signature
InRelease                        # inline-signed metadata
estrella_0.1.0_arm64.deb         # every release is kept
estrella_0.1.1_arm64.deb
estrella_0.2.0_arm64.deb
...
```

The `.deb` files are GitHub Release assets served from GitHub's CDN. apt follows the 302 redirects that GitHub uses for release downloads.

### How CI updates it

Each release:
1. Uploads the new `.deb` to the `apt` release
2. Downloads the existing `Packages` index from the `apt` release
3. Generates a new entry for the new `.deb` and appends it
4. Re-signs and re-uploads the metadata files (overwrites with `--clobber`)

## What the CI does

| Job | Runner | What it does |
|-----|--------|--------------|
| `check` | ubuntu-24.04 | Nix build, frontend build, fmt, clippy, tests, Playwright e2e |
| `helm` | ubuntu-24.04 | `scripts/test-helm-chart.sh` and `scripts/test-next-version.sh` |
| `image` (check) | ubuntu-24.04 | Builds the image and runs `scripts/smoke-test-image.sh` against a fake TCP printer |
| `version` | ubuntu-24.04 | Decides the next version, or that there's nothing to release |
| `image` (release) | ubuntu-24.04 / ubuntu-24.04-arm | Builds and pushes each architecture by digest |
| `image-manifest` | ubuntu-24.04 | Combines the digests into the multi-arch `X.Y.Z` and `X.Y` tags |
| `chart` | ubuntu-24.04 | `helm package` + `helm push` to ghcr.io |
| `build-deb` | ubuntu-24.04-arm | `nix build .#static`, packaged with [nfpm](https://nfpm.goreleaser.com/) |
| `release` | ubuntu-24.04 | Tags `vX.Y.Z` and creates the GitHub Release |
| `publish-apt` | ubuntu-24.04 | Regenerates and signs the apt repository on the `apt` release |

## Verifying the apt repo

After a release, verify the repository is working:

```bash
# Check the package index
curl -sL https://github.com/eljojo/estrella/releases/download/apt/Packages

# Check the GPG key
curl -sL https://github.com/eljojo/estrella/releases/download/apt/gpg.key | gpg --show-keys

# Check the signed release
curl -sL https://github.com/eljojo/estrella/releases/download/apt/InRelease
```

## Testing a .deb locally

```bash
# On a Raspberry Pi (or any arm64 Debian system):
sudo dpkg -i estrella_0.1.0_arm64.deb
sudo apt-get install -f  # installs bluez dependency if needed

# Check installed files
dpkg -L estrella

# Test the binary
estrella --help
```

## Architecture

The `.deb` package contains:
- `/usr/bin/estrella` — statically linked binary (zero runtime deps)
- `/usr/lib/systemd/system/estrella.service` — HTTP daemon
- `/usr/lib/systemd/system/estrella-rfcomm.service` — Bluetooth RFCOMM setup
- `/etc/estrella/estrella.conf` — configuration (survives upgrades)

The binary is built with `--no-default-features` which disables HEIC/HEIF image support (requires libheif C library). Users should convert iPhone photos to JPEG before uploading. All other features work identically to the Nix build. The container image links Debian's libheif and supports HEIC.
