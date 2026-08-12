# rinexfetch

[![CI](https://github.com/MrCry0/rinexfetch/actions/workflows/ci.yml/badge.svg)](https://github.com/MrCry0/rinexfetch/actions/workflows/ci.yml)

RINEX fetch & combine tool for GNSS receiver development labs.

`rinexfetch` is a command-line tool that retrieves RINEX data from NASA's
CDDIS archive for a given time, GNSS constellation set, and set of ground
stations, and produces standards-compliant RINEX 4.xx output. It is intended
for lab use in receiver development, testing, and post-processing. It is
**not** a signal generation or RF transmission tool — it only retrieves and
reformats existing RINEX products.

## Status

Pre-alpha, under active development. See
[`rinexfetch-project-plan.md`](rinexfetch-project-plan.md) for the full
design and the phased development plan; the sections below summarize it.

## What it does

- Fetches a combined multi-GNSS **broadcast navigation (nav)** file for a
  given time, containing ephemerides for the requested constellation(s)
  (GPS / GLONASS / Galileo / BeiDou / QZSS / SBAS / all).
- Optionally fetches per-station **observation (obs)** files for an explicit
  list of ground stations, for the same time and constellation filter.
- Resolves `now`, `yesterday`, or an explicit datetime to the corresponding
  GPS day/session and CDDIS product availability tier.
- Outputs RINEX version 4.xx, upconverting from the source version where
  needed.
- Authenticates against CDDIS via NASA Earthdata Login, with credentials
  sourced through a pluggable provider (interactive prompt or OS-native
  keyring in v1; remote vault backends planned for later).
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

## Usage (planned)

```
rinexfetch --time now|yesterday|<ISO8601> \
           --systems all|gps,glonass,galileo,beidou,qzss,sbas \
           --stations WTZR00DEU,ONSA00SWE,... \
           --rinex-version 4 \
           --output-dir <path>
```

- `--time` — `now` resolves to the most recent time for which a usable nav
  product exists (final, falling back to rapid, then ultra-rapid); `yesterday`
  resolves to the previous UTC day's final combined product; an ISO 8601
  timestamp resolves to its corresponding GPS day/session.
- `--systems` — `all` or a comma-separated subset of `gps`, `glonass`,
  `galileo`, `beidou`, `qzss`, `sbas`; applied as a filter on both the
  combined nav file and any station obs files.
- `--stations` — legacy 4-character or modern 9-character IGS site
  identifiers, normalized internally to 9-character form. Omitted or empty
  means nav-only mode. Unknown or invalid IDs produce a per-station error
  and are skipped rather than aborting the run.
- `--output-dir` — combined nav file plus, if applicable, one obs file per
  successfully resolved station, all in RINEX 4.xx format. Each output file
  is checksum-verified against its source before being considered valid.

## Authentication

CDDIS requires a NASA Earthdata Login (URS) account. `rinexfetch` performs
Basic Auth against `urs.earthdata.nasa.gov` and follows the cookie-jar-based
redirect chain through to `cddis.nasa.gov`. A failed login does not return an
HTTP error status — it returns an HTML login page in place of the requested
file — so the download path validates that retrieved content is actually
gzip/RINEX before treating a request as successful.

Credentials are sourced through a `CredentialProvider` abstraction so new
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
│   ├── time.rs               now/yesterday/datetime → GPS day/session resolution
│   ├── cddis/
│   │   ├── auth.rs           Earthdata Login flow, cookie-jar redirect handling
│   │   ├── discovery.rs      Resolve remote paths for nav & obs products
│   │   └── download.rs       Retrying, resumable, checksum-verified downloads
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
filter, write RINEX 4.xx output, and report a per-file success/failure
summary.

## Reliability

- **`now` fallback tiers**: the final combined nav product typically
  publishes hours to about a day late. `--time now` tries final, then rapid,
  then ultra-rapid products, and labels output with which tier was actually
  used rather than silently serving stale or incomplete data.
- **Per-station isolation**: a failure or unknown ID for one station does
  not abort nav retrieval or other stations' obs retrieval.
- **Checksum verification** on all downloaded files where CDDIS publishes
  one, plus retries with backoff on transient network failures.
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
