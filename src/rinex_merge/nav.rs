//! Multi-GNSS broadcast nav: tries each candidate in priority order (final,
//! then rapid, at each day `--time latest` walks backward through),
//! downloading, decompressing, filtering by requested system, upconverting
//! to RINEX 4.xx if needed, and writing the result.

use std::io::{BufReader, Cursor, Read};
use std::path::{Path, PathBuf};

use flate2::read::GzDecoder;
use reqwest::StatusCode;
use rinex::prelude::{Constellation, Rinex, Version};

use crate::cddis::auth::CddisClient;
use crate::cddis::discovery::{NavCandidate, NavTier};
use crate::cddis::download::{self, DownloadError};
use crate::systems::GnssSystem;
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
}

pub struct NavOutcome {
    pub day: GpsDay,
    pub tier: NavTier,
    pub output_path: PathBuf,
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
    output_dir: &Path,
) -> Result<NavOutcome, NavError> {
    for candidate in candidates {
        match download::download(client, &candidate.url) {
            Ok(gzip_bytes) => {
                let output_path = write_filtered_nav(&gzip_bytes, systems, output_dir)?;
                return Ok(NavOutcome {
                    day: candidate.day,
                    tier: candidate.tier,
                    output_path,
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
    output_dir: &Path,
) -> Result<PathBuf, NavError> {
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

    if rinex.header.version.major < 4 {
        rinex.header = rinex.header.with_version(Version::new(4, 0));
    }

    let filename = rinex.standard_filename(false, None, None);
    let output_path = output_dir.join(filename);
    rinex.to_file(&output_path)?;

    Ok(output_path)
}

fn matches_constellation(system: GnssSystem, constellation: Constellation) -> bool {
    match system {
        GnssSystem::Gps => constellation == Constellation::GPS,
        GnssSystem::Glonass => constellation == Constellation::Glonass,
        GnssSystem::Galileo => constellation == Constellation::Galileo,
        GnssSystem::Beidou => constellation == Constellation::BeiDou,
        GnssSystem::Qzss => constellation == Constellation::QZSS,
        GnssSystem::Sbas => constellation.is_sbas(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gps_matches_only_gps() {
        assert!(matches_constellation(GnssSystem::Gps, Constellation::GPS));
        assert!(!matches_constellation(
            GnssSystem::Gps,
            Constellation::Glonass
        ));
    }

    #[test]
    fn sbas_matches_any_regional_sbas_constellation() {
        assert!(matches_constellation(GnssSystem::Sbas, Constellation::WAAS));
        assert!(matches_constellation(
            GnssSystem::Sbas,
            Constellation::EGNOS
        ));
        assert!(!matches_constellation(GnssSystem::Sbas, Constellation::GPS));
    }
}
