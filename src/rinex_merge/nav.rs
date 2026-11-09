//! Multi-GNSS broadcast nav: tries each candidate in priority order (final,
//! then rapid, at each day `--time latest` walks backward through),
//! downloading, decompressing, filtering by requested system, upconverting
//! to RINEX 4.xx if needed, and writing the result.

use std::fs;
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
use crate::rinex_merge::{convert_version, corrections};
use crate::systems::{GnssSystem, matches_constellation};
use crate::time::GpsDay;
use crate::validate::{self, Report};

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
    /// filtered record. The `rinex` crate's nav writer only formatted
    /// ephemeris frames as of 0.22 (silently, for any target version);
    /// this has not been re-verified against 0.23, so the count is
    /// surfaced here rather than left undetected.
    pub dropped_non_ephemeris: usize,
    /// Result of checking the written file against the RINEX layout and
    /// the source it came from.
    pub validation: Report,
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
        .unwrap_or_else(|payload| {
            Err(NavError::Panicked(crate::rinex_merge::panic_message(
                &payload,
            )))
        });

        match result {
            Ok((output_path, dropped_non_ephemeris, validation)) => {
                return Ok(NavOutcome {
                    day: candidate.day,
                    tier: candidate.tier,
                    output_path,
                    dropped_non_ephemeris,
                    validation,
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

fn write_filtered_nav(
    gzip_bytes: &[u8],
    systems: &[GnssSystem],
    target_version_major: u8,
    output_dir: &Path,
) -> Result<(PathBuf, usize, Report), NavError> {
    let mut decompressed = Vec::new();
    GzDecoder::new(gzip_bytes).read_to_end(&mut decompressed)?;

    let mut rinex = Rinex::parse(&mut BufReader::new(Cursor::new(&decompressed)))?;

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

    if rinex.header.version.major < 4 && target_version_major >= 4 {
        corrections::add_header_corrections(
            &mut rinex,
            systems,
            &String::from_utf8_lossy(&decompressed),
        );
    }
    rinex.header = convert_version(rinex.header, target_version_major);

    let filename = rinex.standard_filename(false, None, None);
    let output_path = output_dir.join(filename);
    if let Err(err) = rinex.to_file(&output_path) {
        if matches!(
            err,
            rinex::prelude::FormattingError::NoNavigationDefinition
                | rinex::prelude::FormattingError::MissingNavigationStandards
        ) {
            return Err(NavError::UnsupportedDownconversion);
        }
        return Err(err.into());
    }

    let source_text = String::from_utf8_lossy(&decompressed);
    let output_text = fs::read_to_string(&output_path)?;
    let validation = validate::nav::validate(&source_text, &output_text, systems);

    Ok((output_path, dropped_non_ephemeris, validation))
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use std::fs;
    use std::io::Write;

    /// One real GPS LNAV record (from a DLR combined nav file) whose
    /// first continuation line starts with a negative value.
    const NAV3: &str = include_str!("../../tests/fixtures/nav3_gps_negative_lead.rnx");

    #[test]
    fn written_nav_has_no_short_negative_continuation_lines() {
        let mut gz = GzEncoder::new(Vec::new(), Compression::default());
        gz.write_all(NAV3.as_bytes()).unwrap();
        let bytes = gz.finish().unwrap();

        let dir = std::env::temp_dir().join("rinexfetch-nav-indent-test");
        fs::create_dir_all(&dir).unwrap();
        let (path, _, _) = write_filtered_nav(&bytes, &[GnssSystem::Gps], 3, &dir).unwrap();

        let written = fs::read_to_string(&path).unwrap();
        fs::remove_file(&path).unwrap();
        for line in written.lines() {
            assert!(!line.starts_with("   -"), "short negative line: {line:?}");
        }
        assert!(written.contains("    -5.010515451431E-06"));
    }

    /// 12 satellites across all seven constellations, cut from a real
    /// DLR combined nav file.
    const MULTI_GNSS: &str = include_str!("../../tests/fixtures/nav3_dlr_multi_gnss.rnx");

    #[test]
    fn rinex_304_continuation_fields_start_after_four_prefix_columns() {
        // First Galileo record of 00CCC_U_20263122330_00U_MN.rnx,
        // whose continuation rows were written with a three-column prefix.
        let malformed = include_str!("../../tests/fixtures/nav3_304_short_indent.rnx");
        assert!(validate::nav::check_structure(malformed).has_errors());
        // Restore the source columns before parsing: parsing the malformed
        // rows first would already lose signs or values.
        let mut body = false;
        let mut source = String::new();
        for line in malformed.lines() {
            if body && line.starts_with(' ') {
                source.push(' ');
            }
            source.push_str(line);
            source.push('\n');
            body |= line.contains("END OF HEADER");
        }
        let dir = std::env::temp_dir().join("rinexfetch-nav-304-columns-test");
        fs::create_dir_all(&dir).unwrap();
        let (path, _, report) =
            write_filtered_nav(&gzip(&source), &crate::systems::ALL_SYSTEMS, 3, &dir).unwrap();
        assert!(!report.has_errors(), "{:?}", report.render());
        let written = fs::read_to_string(&path).unwrap();
        assert!(written.starts_with("     3.04"));
        let rows: Vec<_> = written
            .lines()
            .skip_while(|line| !line.contains("END OF HEADER"))
            .skip(2)
            .collect();
        assert_eq!(rows.len(), 7);
        for row in rows {
            assert_eq!(&row[..4], "    ");
            assert_eq!(row.len(), 80);
        }
        let report = validate::nav::compare(&source, &written, &crate::systems::ALL_SYSTEMS);
        assert!(!report.has_errors(), "{:?}", report.render());
        fs::remove_file(path).unwrap();
        fs::remove_dir(dir).unwrap();
    }

    fn gzip(text: &str) -> Vec<u8> {
        let mut gz = GzEncoder::new(Vec::new(), Compression::default());
        gz.write_all(text.as_bytes()).unwrap();
        gz.finish().unwrap()
    }

    fn ephemeris_count(path: &Path) -> usize {
        let rinex = Rinex::from_file(path).unwrap();
        rinex
            .record
            .as_nav()
            .unwrap()
            .keys()
            .filter(|key| key.frmtype == NavFrameType::Ephemeris)
            .count()
    }

    /// A new directory for each call: tests run in parallel, in threads or,
    /// with nextest, in processes, and the output file name is the same for
    /// every test.
    fn unique_dir(prefix: &str) -> PathBuf {
        static CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let call = CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "rinexfetch-nav-{prefix}-{}-{call}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn frame_count(path: &Path) -> usize {
        let rinex = Rinex::from_file(path).unwrap();
        rinex.record.as_nav().unwrap().len()
    }

    /// The STO and ION frames of the file at `path`: (frame type, system
    /// letter, time system code of a STO frame).
    fn correction_frames(path: &Path) -> Vec<(NavFrameType, char, String)> {
        let rinex = Rinex::from_file(path).unwrap();
        rinex
            .record
            .as_nav()
            .unwrap()
            .iter()
            .filter(|(key, _)| key.frmtype != NavFrameType::Ephemeris)
            .map(|(key, frame)| {
                let code = match frame {
                    rinex::navigation::NavFrame::STO(sto) => {
                        sto.time_system.clone().unwrap_or_default()
                    }
                    _ => String::new(),
                };
                let letter = key.sv.to_string().chars().next().unwrap();
                (key.frmtype, letter, code)
            })
            .collect()
    }

    fn written_path(source: &str, systems: &str, major: u8) -> PathBuf {
        let systems = crate::systems::parse_systems(systems).unwrap();
        let dir = unique_dir("corrections");
        write_filtered_nav(&gzip(source), &systems, major, &dir)
            .unwrap()
            .0
    }

    #[test]
    fn rinex3_header_corrections_are_written_as_records_at_rinex_4() {
        let path = written_path(MULTI_GNSS, "all", 4);
        let frames = correction_frames(&path);
        let header = fs::read_to_string(&path).unwrap();
        fs::remove_file(&path).unwrap();

        // one STO frame per TIME SYSTEM CORR line of the source, GLGP included
        let mut codes: Vec<&str> = frames
            .iter()
            .filter(|(kind, ..)| *kind == NavFrameType::SystemTimeOffset)
            .map(|(_, _, code)| code.as_str())
            .collect();
        codes.sort_unstable();
        assert_eq!(
            codes,
            [
                "BDUT", "GAGP", "GAUT", "GLGP", "GLUT", "GPUT", "IRGA", "IRGL", "IRGP", "IRUT",
                "QZUT"
            ]
        );
        // and one ION frame per constellation with a model in the header
        let mut ions: Vec<char> = frames
            .iter()
            .filter(|(kind, ..)| *kind == NavFrameType::IonosphereModel)
            .map(|(_, letter, _)| *letter)
            .collect();
        ions.sort_unstable();
        assert_eq!(ions, ['C', 'E', 'G', 'I', 'J']);
        assert!(header.contains("> STO R   FDMA"), "{header}");
        assert!(header.contains("GLGP"));
    }

    /// The fixture with two BeiDou model lines per satellite and hour added
    /// to its header, as CDDIS broadcast files have them.
    fn multi_gnss_with_bds_per_satellite() -> String {
        let lines = "BDSA   2.7008E-08  9.6858E-08 -1.0729E-06  1.7285E-06 W 02  IONOSPHERIC CORR\n\
BDSB   1.3926E+05 -7.2090E+05  5.6361E+06 -4.7186E+06 W 02  IONOSPHERIC CORR\n\
BDSA   3.4459E-08  2.2352E-08 -8.3447E-07  1.4901E-06 M 05  IONOSPHERIC CORR\n\
BDSB   1.5770E+05 -8.0282E+05  5.0463E+06 -3.9322E+06 M 05  IONOSPHERIC CORR\n";
        MULTI_GNSS.replacen(
            "                                                            END OF HEADER",
            &format!(
                "{lines}                                                            END OF HEADER"
            ),
            1,
        )
    }

    #[test]
    fn beidou_models_per_satellite_become_one_ion_record_each() {
        let source = multi_gnss_with_bds_per_satellite();
        let path = written_path(&source, "all", 4);
        let rinex = Rinex::from_file(&path).unwrap();
        let report = written_report(&source, 4);
        fs::remove_file(&path).unwrap();

        let mut models: Vec<(u8, (i32, u8, u8, u8))> = rinex
            .record
            .as_nav()
            .unwrap()
            .keys()
            .filter(|key| {
                key.frmtype == NavFrameType::IonosphereModel
                    && key.sv.constellation == rinex::prelude::Constellation::BeiDou
            })
            .map(|key| {
                let (y, m, d, h, ..) = key.epoch.to_gregorian(rinex::prelude::TimeScale::BDT);
                (key.sv.prn, (y, m, d, h))
            })
            .collect();
        models.sort_unstable();
        // the header line without satellite, then the lines of satellites
        // 2 (time mark W, 22h) and 5 (M, 12h) of the day of the file
        assert_eq!(
            models.iter().map(|(prn, _)| *prn).collect::<Vec<_>>(),
            [0, 2, 5]
        );
        assert_eq!(models[1].1, (2025, 4, 13, 22));
        assert_eq!(models[2].1, (2025, 4, 13, 12));
        assert!(!report.has_errors(), "{:?}", report.render());
    }

    #[test]
    fn header_corrections_follow_the_requested_systems() {
        let path = written_path(MULTI_GNSS, "gps", 4);
        let frames = correction_frames(&path);
        fs::remove_file(&path).unwrap();
        let mut kinds: Vec<_> = frames
            .iter()
            .map(|(k, l, c)| (*k, *l, c.as_str()))
            .collect();
        kinds.sort_by_key(|(_, _, code)| *code);
        assert_eq!(
            kinds,
            [
                (NavFrameType::IonosphereModel, 'G', ""),
                (NavFrameType::SystemTimeOffset, 'G', "GPUT"),
            ]
        );
    }

    #[test]
    fn rinex3_output_keeps_header_corrections_in_the_header() {
        let path = written_path(MULTI_GNSS, "all", 3);
        let text = fs::read_to_string(&path).unwrap();
        fs::remove_file(&path).unwrap();
        assert!(text.contains("GLGP"), "{text}");
        assert!(!text.contains("> STO"));
    }

    #[test]
    fn multi_gnss_fixture_keeps_every_frame_at_both_versions() {
        let systems = crate::systems::parse_systems("all").unwrap();
        let bytes = gzip(MULTI_GNSS);
        let dir = std::env::temp_dir().join("rinexfetch-nav-multi-test");
        fs::create_dir_all(&dir).unwrap();

        let source = Rinex::parse(&mut BufReader::new(Cursor::new(MULTI_GNSS))).unwrap();
        let nav = source.record.as_nav().unwrap();
        assert_eq!(nav.len(), 48);
        let expected = nav.len();

        for major in [3u8, 4] {
            let (path, _, _) = write_filtered_nav(&bytes, &systems, major, &dir).unwrap();
            let written = ephemeris_count(&path);
            fs::remove_file(&path).unwrap();
            assert_eq!(written, expected, "RINEX {major} output lost frames");
        }
    }

    /// RINEX 4.02 rapid-tier nav: the first two records of every
    /// (record type, system, message type) combination in a real DLR
    /// file, 64 records covering EPH, STO, ION and EOP frames.
    const RAPID_V4: &str = include_str!("../../tests/fixtures/nav4_dlr_rapid_mixed.rnx");

    #[test]
    fn rapid_v4_fixture_keeps_every_frame_at_rinex_4() {
        let systems = crate::systems::parse_systems("all").unwrap();
        let dir = std::env::temp_dir().join("rinexfetch-nav-rapid-v4-test");
        fs::create_dir_all(&dir).unwrap();

        let source = Rinex::parse(&mut BufReader::new(Cursor::new(RAPID_V4))).unwrap();
        // The rinex crate parses every record of the fixture, the
        // duplicated C06 CNV1/CNV2 pairs included.
        let expected = source.record.as_nav().unwrap().len();
        assert_eq!(expected, 64);

        let (path, _, _) = write_filtered_nav(&gzip(RAPID_V4), &systems, 4, &dir).unwrap();
        let written = frame_count(&path);
        fs::remove_file(&path).unwrap();
        assert_eq!(written, expected, "RINEX 4 output lost frames");
    }

    #[test]
    fn rapid_v4_fixture_downconverts_to_rinex_3() {
        let systems = crate::systems::parse_systems("all").unwrap();
        let dir = std::env::temp_dir().join("rinexfetch-nav-rapid-v3-test");
        fs::create_dir_all(&dir).unwrap();

        let (path, _, _) = write_filtered_nav(&gzip(RAPID_V4), &systems, 3, &dir).unwrap();
        let written = ephemeris_count(&path);
        fs::remove_file(&path).unwrap();
        assert!(written > 0, "RINEX 3 output has no frames");
    }

    fn written_report(source: &str, major: u8) -> Report {
        let systems = crate::systems::parse_systems("all").unwrap();
        let dir = unique_dir("validate");
        let (path, _, report) = write_filtered_nav(&gzip(source), &systems, major, &dir).unwrap();
        fs::remove_file(&path).unwrap();
        report
    }

    #[test]
    fn real_source_fixtures_are_well_formed() {
        for (name, text) in [("multi-gnss v3", MULTI_GNSS), ("rapid v4", RAPID_V4)] {
            let report = validate::nav::check_structure(text);
            assert!(!report.has_errors(), "{name}: {:?}", report.render());
        }
    }

    #[test]
    fn written_multi_gnss_fixture_passes_validation_at_rinex_3() {
        let report = written_report(MULTI_GNSS, 3);
        assert!(!report.has_errors(), "{:?}", report.render());
    }

    #[test]
    fn written_multi_gnss_fixture_passes_validation_at_rinex_4() {
        // a RINEX 3 source: its header corrections must be in the records
        let report = written_report(MULTI_GNSS, 4);
        assert!(!report.has_errors(), "{:?}", report.render());
    }

    #[test]
    fn written_rapid_fixture_passes_validation_at_rinex_4() {
        let report = written_report(RAPID_V4, 4);
        assert!(!report.has_errors(), "{:?}", report.render());
    }
}
