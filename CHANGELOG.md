# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [1.0.2] - 2026-09-08

### Fixed

- Nav output could contain malformed continuation lines (a rinex
  crate formatting bug) that desynced any consumer locating fields by
  column offset, corrupting a large fraction of the orbit data in
  practice.
- The obs pipeline could crash the whole run on a single station's
  data: the rinex crate's CRINEX (Hatanaka) decompressor could
  overflow decompressing certain real, valid receiver-clock offset
  sequences. Per-station panic isolation was added so one station's
  bad input can no longer take down the rest of the run or the nav
  fetch.
- Pinned the rinex dependency to a fork carrying upstream-pending
  fixes for defects found while testing against the live archive:
  the receiver-clock decompression overflow above (root cause, not
  just isolation), a nav writer that omitted every ephemeris
  record's clock and orbital fields, GPS-only RINEX 4 to 3 nav
  downconversion being blocked even where a valid RINEX 3
  representation exists, the obs receiver clock offset never being
  written to output, and RINEX 4.02 (used by CDDIS's rapid nav tier)
  being rejected outright. This is a temporary measure until the
  fixes land in a published rinex release.

## [1.0.1] - 2026-08-14

### Changed

- Generalized the project's stated purpose. Documentation previously
  described rinexfetch as a tool for GNSS receiver development labs;
  it is now described as a general-purpose tool for anyone who needs
  combined RINEX files, for post-processing, analysis, testing, or
  other GNSS work.

### Fixed

- Corrected the README's Reliability section, which incorrectly
  stated that retry/backoff on transient network failures and
  structured logging were implemented. Neither is implemented; the
  section now accurately describes current error handling and notes
  both as outstanding work.

## [1.0.0] - 2026-08-14

First stable release. `rinexfetch` retrieves and combines RINEX GNSS
data from NASA's CDDIS archive for a given time, constellation set, and
set of ground stations.

### Added

- Combined multi-GNSS broadcast navigation (nav) fetch, with automatic
  fallback from the final to the rapid product tier when the final
  product isn't published yet.
- Per-station observation (obs) fetch for an explicit list of ground
  stations, with per-station error isolation so one bad station doesn't
  abort the rest of the run.
- `--time latest` or an explicit ISO 8601 date, resolved to the
  corresponding GPS day and CDDIS product tier.
- Constellation filtering (GPS, GLONASS, Galileo, BeiDou, QZSS, SBAS, or
  all).
- RINEX 3.xx and 4.xx output, converting from the source version where
  needed.
- Authentication against CDDIS via a NASA Earthdata Login (URS) bearer
  token, through a pluggable credential provider: interactive prompt or
  OS-native keyring, with explicit consent before a token is saved to
  the keyring.
- Clear, specific error reporting for authentication failures, missing
  products, unknown or malformed station IDs, and network errors,
  instead of silent empty or wrong output.
- Packaging for Linux (.deb, .rpm), macOS (.pkg, arm64 and x86_64), and
  Windows (.msi), plus plain tarballs/zip archives, all built and
  published automatically from a pushed release tag.
- CI test results published as a per-OS job summary on every pull
  request.
- `CONTRIBUTING.md` describing the fork-and-branch contribution
  workflow and commit conventions.

### Known limitations

- No retry/backoff for transient network failures.
- Downconverting a RINEX-4-native navigation product to RINEX 3 is
  unsupported when only the rapid tier is available, due to a
  limitation in the underlying `rinex` crate rather than a design
  choice of this tool.
- Legacy 4-character station IDs are rejected with a specific error
  rather than auto-expanded to the full 9-character form; expansion
  would need an external station metadata lookup.

See [`rinexfetch-project-plan.md`](rinexfetch-project-plan.md) for the
full design, including further deferred work (remote secret vault
backends, scheduled/daemon mode, full-network obs fetch, and OAuth
token renewal).
