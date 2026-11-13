# Changes in this fork

This fork of [eljojo/estrella](https://github.com/eljojo/estrella) branches from upstream at `f2138e4` (upstream's last release was v0.1.2). It adds ESC/POS network printers, a container image and Helm chart, automated releases and document templates, and fixes a few printing bugs found on real hardware. Everything has been tested on an Xprinter XP-80T (80mm, ESC/POS over TCP). StarPRNT over Bluetooth stays the default and its output is unchanged.

## Printing

**ESC/POS support** (v0.2.0). A second code generator alongside StarPRNT, selected with `--protocol escpos`:
- Text, styles, character sizes, feeds, cuts, 1D barcodes (Code39, Code128, EAN-13, UPC-A, ITF) and QR codes use native commands.
- PDF417, band graphics and built-in logos have no reliable ESC/POS equivalent, so they're rendered by the preview engine and sent as images.
- Init cancels Chinese double-byte mode (many Chinese-market printers boot with it on, which garbles accented and box-drawing characters) and selects code page 437.
- Narrow images are padded to full width by estrella, so their placement doesn't depend on firmware.
- Upperline and reduced printing have no ESC/POS equivalent and are skipped. Storing logos in printer memory stays StarPRNT-only.

**Network printers** (v0.2.0). Raw TCP printing (port 9100 by default) with `--address host[:port]`. `--protocol`, `--address`, `--device` and `--listen` can also be set as `ESTRELLA_*` environment variables.

**Hardware fixes**, each found on the XP-80T:
- *Feeds and positions in the wrong units* (v0.3.1). ESC/POS counts feeds and horizontal positions in "motion units" that default to 1/360 and 1/180 inch, not printer dots, so spacers came out at about 56% of their size. Each job now sets the units to dots first.
- *Space before the cut disappeared* (v0.3.2). The cut command feeds to the cutter counting from the last printed line, which swallowed a separate feed before it. A feed right before a cut now goes inside the cut command.
- *Upside-down text on the wrong side* (v0.4.1). ESC/POS rotates the whole line, so left-aligned upside-down text printed on the right. Left and right are swapped while upside-down is on, to match the preview.
- *Nested markdown lists* (v0.4.1, an upstream bug affecting every printer and the preview). Sub-items printed on their parent's line with large blank gaps after the list, and bulleted sub-lists in numbered lists took the parent's numbers.

**Document options:**
- `"beep": true` sounds the buzzer after printing on ESC/POS printers (v0.3.0). Estrella never beeps otherwise, so a printer that beeps on every receipt is doing so from its own settings.
- QR codes accept `cell_size` up to 16 (was 8), shrunk automatically to fit the paper (v0.4.0). StarPRNT still prints at most 8.

## Templates (v0.4.0, v0.4.1, v0.5.0)

A Templates tab, now the first tab and the one shown on load. It fills JSON document templates from a form and previews and prints them. Templates are `*.json` files in a directory given with `--templates-dir`, re-read on every request. See "Templates" in the README for the format. In short:
- Templates declare fields (`text`, `multiline`, `date`, `url`; optional or required). `{{name}}` placeholders fill from them and the built-in variables. `"when": "field"` drops a component when that field is empty.
- Values are inserted after the JSON is parsed, so they can't break the document.
- Mistakes in a template are reported when it's loaded, with where they are: unknown placeholders, misspelt field types, invalid components.
- Files in the directory that aren't templates are listed in the tab with the reason.
- `examples/templates/` has an example.

## Running it

**Container image** (v0.2.0). `ghcr.io/blybot/estrella`, multi-arch (amd64, arm64):
- Serves the web UI and JSON API on port 8080, as a non-root user, and works with a read-only root filesystem.
- HEIC photo uploads work, using Debian's libheif.
- `GET /healthz` is for probes. It doesn't contact the printer, so a printer that's switched off doesn't take the UI down.

**Helm chart** (v0.2.0). `oci://ghcr.io/blybot/helm/estrella`:
- `printer.protocol` and `printer.address` are required.
- An Ingress is on by default and needs `ingress.host`.
- A values schema rejects unknown or misspelt values.
- `templates.existingConfigMap` mounts a ConfigMap of templates (v0.4.0).
- The API has no authentication, so restrict it at the ingress. The chart README has an example.

## Releases and CI

- **Automatic releases.** Every push to `main` runs the checks, then releases if the commits since the last tag call for it (conventional commits: `feat` → minor, `fix`/`perf`/`refactor` → patch, `!` or `BREAKING CHANGE` → major). A release publishes the image and chart, then tags `vX.Y.Z` and creates a GitHub release. Dependency bumps and `docs:`/`ci:` commits don't release on their own. See `docs/releasing.md`.
- **What gates a release.** The Helm chart tests (`scripts/test-helm-chart.sh`), the release versioning tests, and a container build plus smoke test that prints to a fake TCP printer (`scripts/smoke-test-image.sh`).
- **Dropped from CI.** The Nix-based checks, and with them the `.deb`/apt packaging. Rust tests, clippy, rustfmt and the Playwright tests now run locally only (see `docs/releasing.md` for the commands). The Playwright tests serve `examples/templates`.

## Releases so far

| Version | Highlights |
|---|---|
| v0.2.0 | ESC/POS, TCP printing, env configuration, container image, Helm chart, automated releases |
| v0.3.0 | `"beep": true` |
| v0.3.1 | Feeds and positions in printer dots |
| v0.3.2 | Space before the cut kept |
| v0.4.0 | Templates, larger QR codes, templates ConfigMap in the chart |
| v0.4.1 | Nested markdown lists, upside-down placement, skipped template files explained |
| v0.5.0 | Opens on the Templates tab |
