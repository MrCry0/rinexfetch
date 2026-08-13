# rinexfetch

[![CI](https://github.com/MrCry0/rinexfetch/actions/workflows/ci.yml/badge.svg)](https://github.com/MrCry0/rinexfetch/actions/workflows/ci.yml)

RINEX fetch & combine tool.

`rinexfetch` is a command-line tool that retrieves RINEX data from NASA's
CDDIS archive for a given time, GNSS constellation set, and set of ground
stations, and produces standards-compliant RINEX 3.xx or 4.xx output. It is
intended for lab use in receiver development, testing, and post-processing.
It is **not** a signal generation or RF transmission tool — it only
retrieves and reformats existing RINEX products.

## Status

Pre-alpha, under active development. Both the combined nav pipeline and
per-station obs fetching work end-to-end: auth, `--time latest`/explicit-date
resolution, product discovery (nav's final/rapid fallback; obs has no tier
concept), download, filtering, and RINEX 3.xx/4.xx output, with per-station
error isolation for obs. See
[`rinexfetch-project-plan.md`](rinexfetch-project-plan.md) for the full
design and the phased development plan; the sections below summarize it.

## What it does

- Fetches a combined multi-GNSS **broadcast navigation (nav)** file for a
  given time, containing ephemerides for the requested constellation(s)
  (GPS / GLONASS / Galileo / BeiDou / QZSS / SBAS / all).
- Optionally fetches per-station **observation (obs)** files for an explicit
  list of ground stations, for the same time and constellation filter.
- Resolves `latest` or an explicit datetime to the corresponding GPS
  day/session and CDDIS product availability tier.
- Outputs RINEX version 3.xx or 4.xx (`--rinex-version`, default 4),
  converting from the source version where needed.
- Authenticates against CDDIS with a NASA Earthdata Login (URS) bearer
  token, sourced through a pluggable provider (interactive prompt or
  OS-native keyring in v1; remote vault backends planned for later).
- Detects and clearly reports authentication failures, missing products,
  unknown stations, and network errors, rather than silently producing
  empty or wrong output.

## What it explicitly does not do

- No GNSS signal generation, waveform synthesis, or SDR transmission of any
  kind — this tool only retrieves and combines existing RINEX data files.
- No true single-file merge of observation and navigation data — no RINEX
  version supports this. Output is a combined nav file plus separate
  per-station obs files.
- No scheduled/daemon mode in v1 — one-shot CLI execution only.
- No automatic "world = full IGS/MGEX network" obs mode in v1 — an empty
  station list means nav-only, no obs files are fetched.

## Usage

```
rinexfetch --time latest|<ISO8601> \
           --systems all|gps,glonass,galileo,beidou,qzss,sbas \
           --stations WTZR00DEU,ONSA00SWE,... \
           --rinex-version 3|4 \
           --output-dir <path>
```

- `--time` — `latest` resolves to the most recent time for which a usable
  nav product exists (final, falling back to rapid); an ISO 8601 timestamp
  resolves to its corresponding GPS day/session.
- `--systems` — `all` or a comma-separated subset of `gps`, `glonass`,
  `galileo`, `beidou`, `qzss`, `sbas`; applied as a filter on both the
  combined nav file and any station obs files.
- `--stations` — modern 9-character IGS site identifiers only (e.g.
  `WTZR00DEU`); legacy 4-character IDs aren't auto-expanded (no station
  database lookup in v1) and produce a clear per-station error naming the
  full ID to supply instead. Omitted or empty means nav-only mode. Unknown
  or invalid IDs produce a per-station error and are skipped rather than
  aborting the run.
- `--rinex-version` — `3` or `4` (default `4`); the requested output
  version, converting from the source version where needed.
- `--output-dir` — combined nav file plus, if applicable, one obs file per
  successfully resolved station, all in the requested RINEX version. Each
  source download is integrity-checked via gzip's own CRC32 trailer before
  being considered valid.

## Authentication

CDDIS requires a NASA Earthdata Login (URS) account. `rinexfetch`
authenticates with a URS bearer token, attached as an `Authorization`
header — no username/password exchange or cookie jar involved. Generate a
token at `urs.earthdata.nasa.gov/users/<username>/user_tokens` (valid 60
days, up to 2 active at once). An unauthenticated or invalid-token request
gets a `302` redirect to `urs.earthdata.nasa.gov` instead of the file, which
the download path treats as an auth failure; content-type/magic-byte
validation on the response is kept as a secondary guard.

The token is sourced through a `CredentialProvider` abstraction so new
backends can be added without touching the CDDIS auth logic:

- **v1**: interactive prompt (no echo), or an OS-native keyring (Linux
  Secret Service, macOS Keychain, Windows Credential Manager), falling back
  to an interactive prompt with optional save-to-keyring if nothing is
  stored.
- **planned**: HashiCorp Vault, AWS Secrets Manager, Azure Key Vault / GCP
  Secret Manager, Infisical / Doppler / Bitwarden Secrets Manager, behind
  the same trait.

## Architecture

```
rinexfetch/
├── src/
│   ├── main.rs              CLI entry point, argument parsing
│   ├── time.rs               latest/datetime → GPS day/session resolution
│   ├── stations.rs           --stations validation (9-character IGS IDs)
│   ├── cddis/
│   │   ├── auth.rs           URS bearer-token auth (Authorization header)
│   │   ├── discovery.rs      Resolve remote paths for nav & obs products
│   │   └── download.rs       Retrying, resumable downloads with gzip-integrity checks
│   ├── secrets/
│   │   ├── provider.rs       CredentialProvider trait
│   │   ├── interactive.rs    Interactive prompt backend
│   │   └── keyring.rs        OS-native keyring backend
│   ├── rinex_merge/
│   │   ├── nav.rs             Multi-GNSS nav parse, system-filter, merge, write
│   │   └── obs.rs             Per-station obs parse, system-filter, write
│   └── error.rs               Structured error types (auth / not-yet-published /
│                               network / unknown-station / format)
└── Cargo.toml
```

Data flow: parse CLI args, resolve time to a GPS day/session and product
tier, resolve credentials via the configured `CredentialProvider`, discover
remote CDDIS paths for the nav product and (if stations were given) each
station's obs product, authenticate and download while validating content
type before accepting a response as successful, decompress (gzip, and
Hatanaka decompression for compact RINEX obs), parse and apply the system
filter, write output in the requested RINEX version, and report a per-file
success/failure summary.

## Reliability

- **`latest` fallback tiers**: the final combined nav product (`BRDC00IGS`)
  publishes ~9h after day close; `--time latest` falls back to the DLR
  real-time-stream product (`BRD400DLR`, ~3h after day close) when final
  isn't available yet, and labels output with which tier was actually used
  rather than silently serving stale or incomplete data.
- **Per-station isolation**: a failure or unknown ID for one station does
  not abort nav retrieval or other stations' obs retrieval.
- **Download integrity** via gzip's own CRC32 trailer, validated on
  decompression (CDDIS doesn't publish a separate checksum sidecar for
  these files), plus retries with backoff on transient network failures.
- **Structured logging** distinguishing failure classes (auth /
  not-yet-published / network / unknown-station / parse-format) for lab
  troubleshooting.

## Installation

Prebuilt `.deb` (Debian/Ubuntu), `.rpm` (Fedora), and a plain `x86_64-linux-gnu`
tarball are published on the [releases page][releases] for every tagged
version, alongside a `SHA256SUMS` file to verify downloads against.

```
# Debian / Ubuntu
sudo apt install ./rinexfetch_<version>-1_amd64.deb

# Fedora
sudo dnf install ./rinexfetch-<version>-1.x86_64.rpm
```

[releases]: https://github.com/MrCry0/rinexfetch/releases

## Building

```
cargo build
cargo test
cargo clippy
```

### Testing against the live CDDIS archive

`cargo test` above only runs hermetic tests — no network access, using a
local mock HTTP server to exercise the auth/download classification logic.
Live-network tests that hit the real CDDIS archive live in
[`tests/live_cddis.rs`](tests/live_cddis.rs) and are `#[ignore]`d by
default, so they never run in CI or a plain `cargo test`. They exist
because hand-crafted RINEX fixtures risk validating a bug in the fixture
rather than in the code; testing against real CDDIS data caught several
real issues during development that local mocks couldn't have (see the
project plan for details).

Two tests only check auth-failure classification and need no credentials:

```
cargo test -- --ignored
```

The rest exercise the full nav/obs pipelines against real data and need a
real NASA Earthdata Login (URS) bearer token — generate one at
`urs.earthdata.nasa.gov/users/<username>/user_tokens` (see
[Authentication](#authentication) above) — passed via
`RINEXFETCH_TEST_TOKEN`:

```
RINEXFETCH_TEST_TOKEN="$(cat ~/tmp/urs.token)" \
  cargo test --test live_cddis real_ -- --ignored --nocapture
```

(`real_` matches every nav- and obs-pipeline live test; substitute a more
specific test name to run just one, e.g. `real_obs_product_at_rinex3`.)

### Building packages locally

Packaging is driven by `[package.metadata.deb]` and
`[package.metadata.generate-rpm]` in `Cargo.toml`, via
[`cargo-deb`](https://lib.rs/cargo-deb) and
[`cargo-generate-rpm`](https://lib.rs/cargo-generate-rpm). Both are pure Rust
and don't need `dpkg-deb` or `rpmbuild` installed.

```
cargo install cargo-deb cargo-generate-rpm

cargo build --release
cargo deb --no-build            # -> target/debian/*.deb
cargo generate-rpm              # -> target/generate-rpm/*.rpm
```

## CI & releases

GitHub Actions ([`.github/workflows/ci.yml`](.github/workflows/ci.yml)) runs
`cargo fmt --check`, `cargo clippy`, `cargo build`, and `cargo test` on every
push and pull request, plus a packaging smoke test that builds the `.deb` and
`.rpm` and uploads them as workflow artifacts.

Pushing a tag matching `v*.*.*` runs
[`.github/workflows/release.yml`](.github/workflows/release.yml), which
verifies the tag matches the `Cargo.toml` version, re-runs the test suite,
builds the release binary tarball, `.deb`, and `.rpm`, and publishes them to
a GitHub release with a `SHA256SUMS` file.

## License

GPLv3. See [`LICENSE`](LICENSE).
