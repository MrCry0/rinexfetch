//! Multi-GNSS broadcast nav: tries each candidate in priority order (final,
//! then rapid, at each day `--time latest` walks backward through),
//! downloading, decompressing, filtering by requested system, upconverting
//! to RINEX 4.xx if needed, and writing the result.

use std::io::{BufReader, Cursor, Read};
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};

use flate2::read::GzDecoder;
use reqwest::StatusCode;
use rinex::navigation::NavFrameType;
use rinex::prelude::Rinex;

use crate::cddis::auth::CddisClient;
use crate::cddis::discovery::{NavCandidate, NavTier};
use crate::cddis::download::{self, DownloadError};
use crate::rinex_merge::convert_version;
use crate::systems::{GnssSystem, matches_constellation};
use crate::time::GpsDay;

#[derive(Debug, thiserror::Error)]
pub enum NavError {
    #[error(
        "no usable combined nav product found for the requested day (tried {tried} \
         candidate(s) across final/rapid tiers and fallback days; {malformed} of those \
         downloaded successfully but had unparseable/malformed content)"
    )]
    NotYetPublished { tried: usize, malformed: usize },
    #[error(transparent)]
    Download(#[from] DownloadError),
    #[error("failed to decompress downloaded nav product: {0}")]
    Decompress(#[from] std::io::Error),
    #[error("failed to parse downloaded nav product: {0}")]
    Parse(#[from] rinex::prelude::ParsingError),
    #[error("failed to write nav output: {0}")]
    Write(#[from] rinex::prelude::FormattingError),
    #[error("downloaded product is not a navigation RINEX file")]
    UnexpectedRecordType,
    #[error(
        "cannot represent this nav product in RINEX 3: it contains message types with \
         no RINEX 3 equivalent (try --rinex-version 4)"
    )]
    UnsupportedDownconversion,
    #[error("internal error while parsing/writing this candidate: {0}")]
    Panicked(String),
}

#[derive(Debug)]
pub struct NavOutcome {
    pub day: GpsDay,
    pub tier: NavTier,
    pub output_path: PathBuf,
    /// Count of non-ephemeris nav frames (system time offset, earth
    /// orientation, ionosphere model — all RINEX-4-only) present in the
    /// filtered record. The `rinex` crate's nav writer only formats
    /// ephemeris frames as of 0.22 (silently, for any target version), so
    /// this is surfaced here rather than left undetected.
    pub dropped_non_ephemeris: usize,
}

/// Tries each candidate in order, returning the first that downloads and
/// parses successfully. A `404` on a candidate means that specific product
/// isn't published yet, so the next candidate is tried. A downloaded
/// candidate that fails to parse (`NavError::Parse`), isn't a nav record
/// (`NavError::UnexpectedRecordType`), or triggers a panic inside the
/// `rinex` crate (`NavError::Panicked`) is *also* not treated as fatal —
/// it's skipped in favor of the next candidate, since that failure is
/// about this one candidate's content, not something a different day/tier
/// would necessarily share. Confirmed against the live archive: CDDIS's
/// own merge tooling occasionally produces a malformed combined nav file
/// (a missing newline between two concatenated per-source RINEX headers,
/// for a day observed 2026-08-13), and separately, the `rinex` crate
/// itself can panic (not just return `Err`) on certain nav content (a
/// `KbModel::parse` bounds panic, also observed 2026-08-13) — since
/// `write_filtered_nav` runs on untrusted, externally-controlled CDDIS
/// content, its call is wrapped in `catch_unwind` so a third-party crate
/// panic can't take down the whole process. Any other failure (auth,
/// network, decompression, write) is still fatal immediately, since no
/// other candidate would fix it.
pub fn fetch_and_write(
    client: &CddisClient,
    candidates: &[NavCandidate],
    systems: &[GnssSystem],
    target_version_major: u8,
    output_dir: &Path,
) -> Result<NavOutcome, NavError> {
    let mut malformed = 0;
    for candidate in candidates {
        let gzip_bytes = match download::download(client, &candidate.url) {
            Ok(bytes) => bytes,
            Err(DownloadError::Request(err)) if err.status() == Some(StatusCode::NOT_FOUND) => {
                continue;
            }
            Err(err) => return Err(err.into()),
        };

        let result = panic::catch_unwind(AssertUnwindSafe(|| {
            write_filtered_nav(&gzip_bytes, systems, target_version_major, output_dir)
        }))
        .unwrap_or_else(|payload| Err(NavError::Panicked(panic_message(&payload))));

        match result {
            Ok((output_path, dropped_non_ephemeris)) => {
                return Ok(NavOutcome {
                    day: candidate.day,
                    tier: candidate.tier,
                    output_path,
                    dropped_non_ephemeris,
                });
            }
            Err(
                err @ (NavError::Parse(_) | NavError::UnexpectedRecordType | NavError::Panicked(_)),
            ) => {
                eprintln!(
                    "warning: {:?} tier for day {:04}-{:03} downloaded but is unusable ({err}); \
                     trying the next candidate",
                    candidate.tier, candidate.day.year, candidate.day.day_of_year
                );
                malformed += 1;
                continue;
            }
            Err(err) => return Err(err),
        }
    }
    Err(NavError::NotYetPublished {
        tried: candidates.len(),
        malformed,
    })
}

/// Extracts a human-readable message from a caught panic payload, which is
/// typically a `&str` or `String` (from `panic!`/`.expect()`/etc.) but
/// isn't guaranteed to be either.
fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        message.to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "non-string panic payload".to_string()
    }
}

fn write_filtered_nav(
    gzip_bytes: &[u8],
    systems: &[GnssSystem],
    target_version_major: u8,
    output_dir: &Path,
) -> Result<(PathBuf, usize), NavError> {
    let mut decompressed = Vec::new();
    GzDecoder::new(gzip_bytes).read_to_end(&mut decompressed)?;

    let mut rinex = Rinex::parse(&mut BufReader::new(Cursor::new(decompressed)))?;

    let nav = rinex
        .record
        .as_mut_nav()
        .ok_or(NavError::UnexpectedRecordType)?;
    nav.retain(|key, _| {
        systems
            .iter()
            .any(|system| matches_constellation(*system, key.sv.constellation))
    });

    let dropped_non_ephemeris = nav
        .keys()
        .filter(|key| key.frmtype != NavFrameType::Ephemeris)
        .count();

    rinex.header = convert_version(rinex.header, target_version_major);

    let filename = rinex.standard_filename(false, None, None);
    let output_path = output_dir.join(filename);
    if let Err(err) = rinex.to_file(&output_path) {
        if matches!(
            err,
            rinex::prelude::FormattingError::MissingNavigationStandards
        ) {
            return Err(NavError::UnsupportedDownconversion);
        }
        return Err(err.into());
    }

    Ok((output_path, dropped_non_ephemeris))
}
