//! Multi-GNSS broadcast nav: tries each candidate in priority order (final,
//! then rapid, at each day `--time latest` walks backward through),
//! downloading, decompressing, filtering by requested system, upconverting
//! to RINEX 4.xx if needed, and writing the result.

use std::io::{BufReader, Cursor, Read};
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
        "no combined nav product is published yet for the requested day \
         (tried {tried} candidate(s) across final/rapid tiers and fallback days)"
    )]
    NotYetPublished { tried: usize },
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

/// Tries each candidate in order, returning the first that downloads
/// successfully. A `404` on a candidate means that specific product isn't
/// published yet, so the next candidate is tried; any other failure (auth,
/// network, content validation) is fatal immediately, since no other
/// candidate would fix it.
pub fn fetch_and_write(
    client: &CddisClient,
    candidates: &[NavCandidate],
    systems: &[GnssSystem],
    target_version_major: u8,
    output_dir: &Path,
) -> Result<NavOutcome, NavError> {
    for candidate in candidates {
        match download::download(client, &candidate.url) {
            Ok(gzip_bytes) => {
                let (output_path, dropped_non_ephemeris) =
                    write_filtered_nav(&gzip_bytes, systems, target_version_major, output_dir)?;
                return Ok(NavOutcome {
                    day: candidate.day,
                    tier: candidate.tier,
                    output_path,
                    dropped_non_ephemeris,
                });
            }
            Err(DownloadError::Request(err)) if err.status() == Some(StatusCode::NOT_FOUND) => {
                continue;
            }
            Err(err) => return Err(err.into()),
        }
    }
    Err(NavError::NotYetPublished {
        tried: candidates.len(),
    })
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
