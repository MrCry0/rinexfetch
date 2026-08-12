//! Per-station observation: downloads, decompresses (gzip, plus Hatanaka
//! for compact RINEX — handled transparently by `Rinex::parse`), filters
//! by requested system, converts to the requested RINEX version, and
//! writes. Per-station error isolation: one station's failure is reported
//! and skipped, never aborting the others or the nav fetch (plan §4.3,
//! §8).

use std::io::{BufReader, Cursor, Read};
use std::path::{Path, PathBuf};

use flate2::read::GzDecoder;
use reqwest::StatusCode;
use rinex::prelude::Rinex;

use crate::cddis::auth::CddisClient;
use crate::cddis::discovery;
use crate::cddis::download::{self, DownloadError};
use crate::rinex_merge::convert_version;
use crate::stations::{self, StationIdError};
use crate::systems::{GnssSystem, matches_constellation};
use crate::time::GpsDay;

#[derive(Debug, thiserror::Error)]
pub enum ObsError {
    #[error(transparent)]
    InvalidStationId(#[from] StationIdError),
    #[error(
        "no obs product is published for this station on this day (unknown \
         station, or it simply didn't submit data that day)"
    )]
    NotFound,
    #[error(transparent)]
    Download(#[from] DownloadError),
    #[error("failed to decompress downloaded obs product: {0}")]
    Decompress(#[from] std::io::Error),
    #[error("failed to parse downloaded obs product: {0}")]
    Parse(#[from] rinex::prelude::ParsingError),
    #[error("failed to write obs output: {0}")]
    Write(#[from] rinex::prelude::FormattingError),
    #[error("downloaded product is not an observation RINEX file")]
    UnexpectedRecordType,
}

#[derive(Debug)]
pub struct StationOutcome {
    pub station: String,
    pub result: Result<PathBuf, ObsError>,
}

/// Fetches and writes obs data for every requested station, isolating
/// failures per station: a bad station ID, an unpublished/unknown
/// station, or any other per-station error is captured in that station's
/// `StationOutcome` rather than short-circuiting the rest of the list.
pub fn fetch_and_write_all(
    client: &CddisClient,
    day: GpsDay,
    stations: &[String],
    systems: &[GnssSystem],
    target_version_major: u8,
    output_dir: &Path,
) -> Vec<StationOutcome> {
    stations
        .iter()
        .map(|raw_station| StationOutcome {
            station: raw_station.clone(),
            result: fetch_and_write_one(
                client,
                day,
                raw_station,
                systems,
                target_version_major,
                output_dir,
            ),
        })
        .collect()
}

fn fetch_and_write_one(
    client: &CddisClient,
    day: GpsDay,
    raw_station: &str,
    systems: &[GnssSystem],
    target_version_major: u8,
    output_dir: &Path,
) -> Result<PathBuf, ObsError> {
    let station = stations::validate_station_id(raw_station)?;
    let url = discovery::obs_url(day, &station);

    let gzip_bytes = match download::download(client, &url) {
        Ok(bytes) => bytes,
        Err(DownloadError::Request(err)) if err.status() == Some(StatusCode::NOT_FOUND) => {
            return Err(ObsError::NotFound);
        }
        Err(err) => return Err(err.into()),
    };

    let mut decompressed = Vec::new();
    GzDecoder::new(gzip_bytes.as_slice()).read_to_end(&mut decompressed)?;

    let mut rinex = Rinex::parse(&mut BufReader::new(Cursor::new(decompressed)))?;

    let obs = rinex
        .record
        .as_mut_obs()
        .ok_or(ObsError::UnexpectedRecordType)?;
    obs.retain(|_key, observations| {
        observations.signals.retain(|signal| {
            systems
                .iter()
                .any(|system| matches_constellation(*system, signal.sv.constellation))
        });
        !observations.signals.is_empty() || observations.clock.is_some()
    });

    rinex.header = convert_version(rinex.header, target_version_major);
    if let Some(obs_header) = rinex.header.obs.as_mut() {
        // Force plain (uncompressed) RINEX text output rather than
        // re-emitting Compact RINEX (CRINEX), matching the nav path's
        // plain-text output and the plan's stated data flow.
        obs_header.crinex = None;
    }

    let filename = rinex.standard_filename(false, None, None);
    let output_path = output_dir.join(filename);
    rinex.to_file(&output_path)?;

    Ok(output_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::systems::ALL_SYSTEMS;

    #[test]
    fn invalid_station_ids_are_isolated_and_never_hit_the_network() {
        // validate_station_id fails before any request is built, so this
        // never touches the network — a dummy token/client is fine, and
        // this test needs no #[ignore]/live archive access.
        let client = CddisClient::new("unused".to_string()).unwrap();
        let day = GpsDay::resolve("2026-08-01").unwrap();
        let stations = vec!["WTZR".to_string(), "not-a-station-id".to_string()];

        let outcomes = fetch_and_write_all(
            &client,
            day,
            &stations,
            &ALL_SYSTEMS,
            4,
            std::path::Path::new("/nonexistent"),
        );

        assert_eq!(outcomes.len(), 2);
        assert!(matches!(
            outcomes[0].result,
            Err(ObsError::InvalidStationId(
                StationIdError::LegacyFourCharacterId(_)
            ))
        ));
        assert!(matches!(
            outcomes[1].result,
            Err(ObsError::InvalidStationId(StationIdError::InvalidFormat(_)))
        ));
    }
}
