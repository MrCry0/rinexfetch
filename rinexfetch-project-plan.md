# RINEX Fetch & Combine Tool — Project Description & Development Plan

## 1. Purpose

A Rust command-line tool for a GNSS receiver development lab that retrieves and
combines RINEX data from NASA's CDDIS archive for a specified time, GNSS
constellation set, and set of ground stations. Output is standards-compliant
RINEX 3.xx or 4.xx data (selectable via `--rinex-version`) intended for lab
use in receiver development, testing, and post-processing — not for signal
generation or RF transmission. Signal synthesis is explicitly out of scope
for this tool.

## 2. Goals

- Fetch a **combined multi-GNSS broadcast navigation (nav) file** for a given
  time, containing ephemerides for the requested constellation(s)
  (GPS / GLONASS / Galileo / BeiDou / QZSS / SBAS / all).
- Optionally fetch **per-station observation (obs) files** for an explicit
  list of ground stations, for the same time and constellation filter.
- Support time selection as `latest` or an explicit datetime, correctly
  resolved to the corresponding GPS day/session and CDDIS product
  availability tier.
- Output RINEX version 3.xx or 4.xx, selectable via `--rinex-version`,
  converting from the source version where needed (up or down).
- Authenticate against CDDIS via NASA Earthdata Login, with credentials
  sourced through a pluggable provider: interactive prompt or OS-native
  keyring in v1, with remote vault backends (HashiCorp Vault, AWS Secrets
  Manager, Infisical, etc.) added later behind the same interface.
- Be reliable: detect and clearly report authentication failures, missing
  products, unknown stations, and network errors rather than silently
  producing empty or wrong output.

## 3. Non-Goals

- No GNSS signal generation, waveform synthesis, or SDR transmission of any
  kind. This tool only retrieves and combines existing RINEX data files.
- No true single-file merge of observation and navigation data — this is not
  supported by any RINEX version. Output is a combined nav file plus separate
  per-station obs files.
- No scheduled/daemon mode in v1 — one-shot CLI execution only.
- No automatic "world = full IGS/MGEX network" obs mode in v1 — an empty
  station list means nav-only, no obs files fetched.

## 4. Functional Requirements

### 4.1 Time selection
- `--time latest` — resolves to the most recent time for which a usable
  nav product exists (see §8 fallback tiers).
- `--time <ISO8601>` — resolves to the corresponding GPS day/session.

### 4.2 System selection
- `--systems all` or a comma-separated subset: `gps`, `glonass`, `galileo`,
  `beidou`, `qzss`, `sbas`.
- Applied as a filter when writing the combined nav file and when writing
  station obs files (observations for systems not requested are dropped from
  the output).

### 4.3 Station / region selection
- `--stations <id,id,...>` — accepts the modern 9-character IGS site
  identifier form (`XXXX##CCC`) only. Legacy 4-character IDs are not
  auto-expanded (see §11 Phase 5, §12) and produce a specific per-station
  error naming the full ID to supply instead.
- Omitted or empty list → **nav-only mode**: no obs files are fetched, only
  the combined nav file is produced.
- Unknown or invalid station IDs produce a per-station error and are skipped;
  they do not abort the overall run.

### 4.4 Output
- `--output-dir <path>` — combined nav file plus, if applicable, one obs file
  per successfully resolved station, all in the requested RINEX version
  (`--rinex-version 3` or `4`, default `4`).
- Each output file's source download is integrity-checked before being
  considered valid: gzip's own CRC32 trailer, validated on decompression
  (CDDIS doesn't publish a separate checksum sidecar for these files; see
  §8).

## 5. Authentication

CDDIS requires a NASA Earthdata Login (URS) account, and its archive access
control is an OAuth2 flow. This was confirmed empirically against the live
archive rather than assumed from documentation:

- An unauthenticated `GET` on a protected file returns `302 Found` with
  `Location: https://urs.earthdata.nasa.gov/oauth/authorize?...` — CDDIS's
  browser-facing login redirect.
- The same request with an `Authorization: Bearer <token>` header, using a
  URS user token generated at
  `urs.earthdata.nasa.gov/users/<username>/user_tokens`, is served the file
  directly with no further exchange.
- A `HEAD` request bypasses this check entirely on at least some paths
  (returns `200` with or without a token) — an archive quirk, not something
  to rely on. All auth verification must use `GET`.

Because a token is sufficient and is trivial to attach (a single header, no
session state), v1 authenticates with a URS bearer token only. It does not
implement the full OAuth authorization-code exchange (Basic Auth against
`urs.earthdata.nasa.gov` + cookie-jar redirect handling) that would be needed
to turn a username/password into a session — that exchange is deferred (see
§12) since a token is both sufficient and far cheaper to implement correctly.

The download path still validates that retrieved content is actually
gzip/RINEX before treating a request as successful, as defense in depth: the
redirect-based failure above is easy to detect on status/`Location` alone,
but this guards against any other unexpected response shape.

## 6. Secrets Management

### 6.1 Design
A `CredentialProvider` trait abstracts token retrieval, with concrete
backends selected by configuration or CLI flag. This allows new backends to
be added without touching the CDDIS auth logic.

### 6.2 v1 backends
- **Interactive** — prompt for a URS bearer token at runtime (no echo).
- **OS-native keyring** — Linux Secret Service, macOS Keychain, Windows
  Credential Manager, via a cross-platform keyring library, storing the
  token. Falls back to interactive prompt (with optional save-to-keyring) if
  no stored token is found. Note tokens expire after 60 days, so a stored
  token can go stale between runs.

### 6.3 Future backends (not in v1, same trait)
- HashiCorp Vault
- AWS Secrets Manager
- Azure Key Vault / GCP Secret Manager
- Infisical / Doppler / Bitwarden Secrets Manager

## 7. Architecture

```
rinexfetch/
├── src/
│   ├── main.rs              CLI entry point, argument parsing
│   ├── time.rs               latest/datetime → GPS day/session resolution
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

### 7.1 Data flow
1. Parse CLI arguments.
2. Resolve time input to GPS day/session and CDDIS product tier.
3. Resolve a bearer token via the configured `CredentialProvider`.
4. Discover remote paths for the combined nav product and (if stations given)
   each station's obs product.
5. Download with the token attached as an `Authorization` header, validating
   content type before accepting a response as successful.
6. Decompress (gzip, and Hatanaka decompression for compact RINEX obs).
7. Parse, apply system filter, and write RINEX 4.xx output.
8. Report per-file success/failure summary.

## 8. Reliability Considerations

- **`latest` fallback tiers**: two real tiers exist for the combined
  multi-GNSS broadcast nav product, not three — "final/rapid/ultra-rapid"
  is IGS orbit/clock-product terminology that doesn't apply to broadcast
  nav. Confirmed against the live archive by comparing `Last-Modified`
  timestamps for a settled day: `BRDC00IGS_R_YYYYDDD0000_01D_MN.rnx.gz`
  (IGS-combined, "final") publishes ~9h after day close;
  `BRD400DLR_S_YYYYDDD0000_01D_MN.rnx.gz` (DLR real-time-stream combined,
  "rapid", already RINEX 4 so no upconversion needed) publishes ~3h after
  day close, roughly 6h earlier. `--time latest` tries final, then falls
  back to rapid, clearly labeling the output with which tier was actually
  used rather than silently serving stale or incomplete data. No lower-latency
  *combined* product exists to serve as a third tier — the only faster
  CDDIS nav data is per-station 15-minute component files, not a combined
  product, and out of scope.
- **Auth failure detection**: an unauthenticated/invalid-token request is
  caught by the `302`-to-`urs.earthdata.nasa.gov` redirect (see §5); content
  type/magic-byte checking is kept as a secondary guard, not the primary
  signal.
- **Per-station isolation**: a failure or unknown ID for one station does not
  abort nav retrieval or other stations' obs retrieval.
- **Checksum verification** on all downloaded files where CDDIS publishes
  one.
- **Retries with backoff** on transient network failures.
- **Structured logging** distinguishing failure classes (auth / not-yet-
  published / network / unknown-station / parse-format) for lab
  troubleshooting.

## 9. Key Dependencies (proposed)

| Purpose | Crate |
|---|---|
| RINEX 2/3/4 parsing & writing | `rinex` |
| GNSS/GPS time handling | `hifitime` |
| HTTP client | `reqwest` |
| OS-native credential storage | `keyring` |
| Interactive credential prompt | `rpassword` / `dialoguer` |
| CLI argument parsing | `clap` |
| Structured logging | `tracing` |
| Error handling | `thiserror` / `anyhow` |
| Compression | `flate2` (gzip); Hatanaka (CRX2RNX) via the `rinex` crate's transparent CRINEX support in `Rinex::parse`, no separate library needed |

## 10. CLI Usage (proposed)

```
rinexfetch --time latest|<ISO8601> \
           --systems all|gps,glonass,galileo,beidou,qzss,sbas \
           --stations WTZR00DEU,ONSA00SWE,... \
           --rinex-version 3|4 \
           --output-dir <path>
```

## 11. Development Plan

### Phase 1 — Foundations
- Cargo project scaffold, module skeleton, CLI argument parsing.
- Time resolution logic (`latest` / datetime → GPS day/session).
- `CredentialProvider` trait plus interactive backend.

### Phase 2 — CDDIS Authentication
- URS bearer-token auth: attach `Authorization: Bearer <token>` to CDDIS
  requests (`reqwest`, no cookie jar needed).
- Redirect/status-based auth-failure detection, plus content-type validation
  as a secondary guard against unexpected response shapes.
- OS-native keyring backend, including save-on-first-successful-auth flow.

### Phase 3 — Nav Pipeline (end-to-end path 1)
- CDDIS path discovery for the combined broadcast nav product, including
  the final/rapid fallback tiers (see §8).
- Download, decompression; gzip's own CRC32 trailer (validated on
  decompression) serves as download-integrity verification, since CDDIS
  doesn't publish a separate checksum sidecar for these files.
- Parse via the `rinex` crate, apply system filter, write RINEX 4.xx nav
  output.

### Phase 4 — RINEX Version Selection
- `--rinex-version` accepts `3` or `4` (rejects anything else), replacing
  the Phase 1 stub that only accepted `4`.
- Nav output targets whichever version was requested: convert up (3→4,
  already implemented in Phase 3) via the `rinex` crate's
  `Header::with_version`, or pass through unchanged when the source is
  already the requested version.
- **4→3 downconversion is not achievable with the `rinex` crate as of
  0.22** when the source is RINEX-4-native (`BRD400DLR`, the rapid tier):
  confirmed against the live archive, the crate's nav writer returns
  `FormattingError::MissingNavigationStandards` for such records — tried
  with both the full constellation set and GPS-only, same result for both,
  so this isn't a narrow per-constellation gap. rinexfetch surfaces this as
  `NavError::UnsupportedDownconversion`, a clear, specific error suggesting
  `--rinex-version 4`, rather than attempting a silent partial write. Note
  this only affects the rapid tier: when the final tier (`BRDC00IGS`,
  already RINEX 3) is available, `--rinex-version 3` is a same-version
  passthrough with no conversion attempted, so it always works.
- Non-ephemeris nav frames (system time offset / earth orientation /
  ionosphere model — all RINEX-4-only) are silently dropped by the `rinex`
  crate's nav writer regardless of target version, as of 0.22 (its
  formatter only handles `NavFrame::EPH`). rinexfetch counts these before
  writing and reports the count, rather than leaving the gap undetected.
- Land this before Phase 5 (Obs Pipeline) so the obs pipeline is built
  against the final version-selection design from the start, rather than
  needing a follow-up rework once obs output also has to respect
  `--rinex-version`.

### Phase 5 — Obs Pipeline
- Station ID validation: only the modern 9-character IGS form
  (`XXXX##CCC`) is accepted. Legacy 4-character IDs are **not**
  auto-expanded to 9 characters — the 3-letter country-code suffix isn't
  derivable from the site code alone without an external station database,
  which is out of scope for v1 (see §12). A 4-character ID gets a specific,
  actionable per-station error rather than a silent guess.
- CDDIS path discovery for per-station obs products. Confirmed against the
  live archive: obs has no tier concept like nav's final/rapid — CDDIS
  publishes one daily file per station, straight from that station's own
  receiver submission (`R` source flag).
- Download, decompress (gzip, plus Hatanaka for compact RINEX — handled
  transparently by the `rinex` crate's parser, no separate decompression
  step needed), parse, system filter, convert to the requested RINEX
  version, write. Unlike nav, obs version conversion was tested in both
  directions against real station data and did not hit the nav pipeline's
  `MissingNavigationStandards` limitation.
- Per-station error isolation and reporting: each station's outcome
  (success or a specific error — invalid ID, not found/unpublished,
  network, parse/format) is captured independently; one station's failure
  never aborts the others or the nav fetch.

### Phase 6 — Reliability Hardening
- Retry/backoff logic for transient network failures.
- Structured error classification and logging.
- Run summary reporting (per-file success/failure, product tier used, etc.).
- Test fixtures against known CDDIS products for regression testing.

### Phase 7 — Documentation & Handoff
- Usage documentation.
- Notes on adding new `CredentialProvider` backends (Vault, AWS, etc.) for
  future work.

## 12. Open Questions / Future Work

- Remote vault backends (HashiCorp Vault, AWS Secrets Manager, Infisical,
  etc.) — deferred past v1, same trait.
- Scheduled/daemon execution mode — deferred past v1.
- Full "world = all IGS/MGEX stations" obs mode — deferred past v1; current
  scope requires an explicit station list for any obs retrieval.
- Obs RINEX version conversion (Phase 5) was validated against a real
  station (`WTZR00DEU`, a settled day) in both directions and worked
  cleanly, unlike nav's 4→3 case below — no `rinex` crate limitation hit.
  Broader edge cases (2.11 obs-type mapping specifically, other stations'
  receiver/firmware quirks) haven't been exhaustively tested; revisit if a
  real station turns out to hit a similar formatting gap.
- 4→3 nav downconversion from a RINEX-4-native source is unsupported (see
  Phase 4) due to a `rinex` crate limitation, not a rinexfetch design
  choice. Deferred past v1: either contribute a fix upstream, or implement
  nav-message reformatting ourselves if it becomes a real blocker (in
  practice it only bites when `--rinex-version 3` is requested and the
  final tier isn't published yet, so the rapid tier is the only source).
- 4-character legacy station ID auto-expansion to the 9-character form
  (Phase 5) is deferred past v1: the 3-letter country-code suffix isn't
  derivable from the site code alone, and would need an external station
  database (e.g. the IGS M3G metadata service) to resolve — a real scope
  addition, not a quick fix. v1 requires the full 9-character ID and
  reports a specific, actionable error for a 4-character one instead of
  guessing.
- Username/password authentication via the full URS OAuth authorization-code
  exchange (Basic Auth + cookie-jar redirect handling) — deferred past v1;
  a bearer token is sufficient and much cheaper to implement correctly, and
  generating one is a one-time manual step at
  `urs.earthdata.nasa.gov/users/<username>/user_tokens`.
- URS tokens expire after 60 days. v1 surfaces an expired/invalid token as a
  clear auth error (via the `302`-redirect check) rather than auto-renewing;
  automatic renewal would require the OAuth exchange above.
