//! Live-network sanity checks against the real CDDIS archive. Not run in CI
//! (network-dependent; `cddis::auth`'s unit tests cover the classification
//! logic hermetically against a local server). Run manually with
//! `cargo test -- --ignored`; the full-pipeline test additionally needs a
//! real bearer token in `RINEXFETCH_TEST_TOKEN`.

use std::fs;
use std::ops::Deref;
use std::path::{Path, PathBuf};

use rinexfetch::cddis::auth::{CddisAuthError, CddisClient};
use rinexfetch::cddis::discovery::{self, NavTier};
use rinexfetch::rinex_merge::nav;
use rinexfetch::rinex_merge::obs;
use rinexfetch::systems::{ALL_SYSTEMS, GnssSystem};
use rinexfetch::time::GpsDay;

/// Creates `std::env::temp_dir()/name` and removes it on drop. Most of
/// these tests end with an assertion, and a plain `fs::remove_dir_all` at
/// the end of the function body is skipped whenever an earlier assertion
/// fails/panics — this makes cleanup unconditional instead.
struct TestOutputDir(PathBuf);

impl TestOutputDir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(name);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Deref for TestOutputDir {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestOutputDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
#[ignore = "hits the live CDDIS archive"]
fn garbage_token_is_rejected_by_real_cddis() {
    let client = CddisClient::new("not-a-real-token".to_string()).unwrap();
    let err = client.verify_token().unwrap_err();
    // A syntactically-present but invalid token gets a direct 401 from
    // CDDIS, not a redirect (that's the no-token-at-all case).
    assert!(matches!(err, CddisAuthError::InvalidToken { .. }));
}

#[test]
#[ignore = "hits the live CDDIS archive"]
fn missing_token_is_rejected_by_real_cddis() {
    let client = CddisClient::new(String::new()).unwrap();
    let err = client.verify_token().unwrap_err();
    assert!(matches!(err, CddisAuthError::Unauthenticated { .. }));
}

#[test]
#[ignore = "hits the live CDDIS archive; needs RINEXFETCH_TEST_TOKEN"]
fn real_final_nav_product_downloads_filters_and_upconverts() {
    let token = std::env::var("RINEXFETCH_TEST_TOKEN")
        .expect("set RINEXFETCH_TEST_TOKEN to a real URS bearer token to run this test");
    let client = CddisClient::new(token).unwrap();

    // A settled, long-archived day so the final product is guaranteed to
    // already be published.
    let day = GpsDay::resolve("2026-08-01").unwrap();
    let candidates = discovery::nav_candidates_for_day(day);

    let output_dir = TestOutputDir::new("rinexfetch-live-test-nav");

    let outcome = nav::fetch_and_write(&client, &candidates, &ALL_SYSTEMS, 4, &output_dir)
        .expect("fetch_and_write should succeed against a real, settled day");

    assert!(outcome.output_path.exists());

    // Re-parse our own output to confirm it's valid, and actually RINEX 4
    // as promised (the input for the "final" tier is RINEX 3).
    let written = rinex::prelude::Rinex::from_file(&outcome.output_path)
        .expect("written output should itself be valid RINEX");
    assert_eq!(written.header.version.major, 4);
    assert!(
        written.record.as_nav().is_some_and(|nav| !nav.is_empty()),
        "filtered nav record should not be empty for --systems all"
    );
}

#[test]
#[ignore = "hits the live CDDIS archive; needs RINEXFETCH_TEST_TOKEN"]
fn real_final_nav_product_passes_through_at_rinex3() {
    let token = std::env::var("RINEXFETCH_TEST_TOKEN")
        .expect("set RINEXFETCH_TEST_TOKEN to a real URS bearer token to run this test");
    let client = CddisClient::new(token).unwrap();

    // The final tier (BRDC00IGS) is already RINEX 3, so requesting
    // --rinex-version 3 should be a same-version passthrough: no
    // conversion attempted, no risk of hitting the 4->3 limitation below.
    let day = GpsDay::resolve("2026-08-01").unwrap();
    let candidates = discovery::nav_candidates_for_day(day);

    let output_dir = TestOutputDir::new("rinexfetch-live-test-nav-v3-passthrough");

    let outcome = nav::fetch_and_write(&client, &candidates, &ALL_SYSTEMS, 3, &output_dir)
        .expect("same-version (3 -> 3) passthrough should always succeed");

    let written = rinex::prelude::Rinex::from_file(&outcome.output_path)
        .expect("written output should itself be valid RINEX");
    assert_eq!(written.header.version.major, 3);
}

#[test]
#[ignore = "hits the live CDDIS archive; needs RINEXFETCH_TEST_TOKEN"]
fn real_rapid_nav_downconverts_gps_ephemeris_to_rinex3() {
    let token = std::env::var("RINEXFETCH_TEST_TOKEN")
        .expect("set RINEXFETCH_TEST_TOKEN to a real URS bearer token to run this test");
    let client = CddisClient::new(token).unwrap();

    let day = GpsDay::resolve("2026-08-01").unwrap();
    // Force the rapid (BRD400DLR, RINEX 4) tier by excluding final, so this
    // actually exercises the 4 -> 3 downconversion path rather than a
    // same-version passthrough.
    let candidates: Vec<_> = discovery::nav_candidates_for_day(day)
        .into_iter()
        .filter(|candidate| candidate.tier == NavTier::Rapid)
        .collect();
    assert_eq!(candidates.len(), 1);

    let output_dir = TestOutputDir::new("rinexfetch-live-test-nav-v3-downconvert");

    // GPS-only downconversion of a RINEX-4-tagged nav product to RINEX 3
    // used to be impossible (rinex 0.22.0 returned
    // NavError::UnsupportedDownconversion unconditionally, for every
    // constellation tried, GPS included). That's since been fixed: GPS
    // LNAV ephemeris has a real RINEX 3 representation, so it's now
    // written out correctly, rather than either failing outright or
    // (an intermediate, worse regression seen along the way) silently
    // reporting success while writing data-free, unparseable output.
    // Confirmed here by checking real orbital field content, not just
    // that the call returned Ok.
    let outcome = nav::fetch_and_write(&client, &candidates, &[GnssSystem::Gps], 3, &output_dir)
        .expect("GPS-only downconversion to RINEX 3 should succeed now");

    let written = rinex::prelude::Rinex::from_file(&outcome.output_path)
        .expect("written output should itself be valid RINEX");
    assert_eq!(written.header.version.major, 3);
    let nav = written
        .record
        .as_nav()
        .expect("filtered record should still be a nav record");
    assert!(!nav.is_empty(), "GPS ephemeris should not be empty");
    assert!(
        nav.values().any(|frame| frame
            .as_ephemeris()
            .is_some_and(|eph| eph.get_orbit_f64("sqrta").is_some_and(|a| a > 1000.0))),
        "at least one ephemeris should carry a real sqrt(A) orbital parameter, not just an epoch"
    );
}

#[test]
#[ignore = "hits the live CDDIS archive; needs RINEXFETCH_TEST_TOKEN"]
fn real_obs_product_downloads_and_writes_for_known_station() {
    let token = std::env::var("RINEXFETCH_TEST_TOKEN")
        .expect("set RINEXFETCH_TEST_TOKEN to a real URS bearer token to run this test");
    let client = CddisClient::new(token).unwrap();

    let day = GpsDay::resolve("2026-08-01").unwrap();
    let output_dir = TestOutputDir::new("rinexfetch-live-test-obs");

    let outcomes = obs::fetch_and_write_all(
        &client,
        day,
        &["WTZR00DEU".to_string()],
        &ALL_SYSTEMS,
        4,
        &output_dir,
    );

    assert_eq!(outcomes.len(), 1);
    let output_path = outcomes[0]
        .result
        .as_ref()
        .expect("WTZR00DEU should have obs data for a settled day")
        .clone();

    let written = rinex::prelude::Rinex::from_file(&output_path)
        .expect("written output should itself be valid RINEX");
    assert_eq!(written.header.version.major, 4);
    assert!(
        written.record.as_obs().is_some_and(|obs| !obs.is_empty()),
        "filtered obs record should not be empty for --systems all"
    );
}

#[test]
#[ignore = "hits the live CDDIS archive; needs RINEXFETCH_TEST_TOKEN"]
fn real_obs_decompresses_previously_overflowing_clock_data() {
    let token = std::env::var("RINEXFETCH_TEST_TOKEN")
        .expect("set RINEXFETCH_TEST_TOKEN to a real URS bearer token to run this test");
    let client = CddisClient::new(token).unwrap();

    // Regression test for a real bug (2026-09-02): the rinex crate's
    // receiver-clock CRINEX (Hatanaka) decompressor hit a genuine i64
    // overflow on this station's real, uncorrupted clock-offset sequence
    // for this day (verified directly against the raw decompressed bytes,
    // independent of this crate's own error handling, before concluding
    // it was a crate defect rather than corrupted CDDIS content). Also
    // reported independently upstream against a different station's data
    // (nav-solutions/rinex#426) and root-caused to an off-by-one in
    // NumDiff::rotate_history that left the oldest history slot frozen at
    // its initial value.
    //
    // Now that the fix is pinned, this station's data must decompress
    // successfully with real observation content rather than merely "not
    // panic" — asserting panic-isolation alone would have made this test
    // pass again the moment the crate reverted to producing silently
    // wrong (wrapped, not overflowed) values in a release build instead
    // of panicking, which is the failure mode `panic::catch_unwind` can't
    // catch at all. Note: this station's decompressed output currently
    // has no receiver-clock field populated in the parsed record (a
    // separate, unrelated gap from the overflow fix), so this checks the
    // signal data that is populated rather than the clock offset itself.
    let day = GpsDay::resolve("2026-08-31").unwrap();
    let output_dir = TestOutputDir::new("rinexfetch-live-test-obs-clock-overflow-fix");

    let outcomes = obs::fetch_and_write_all(
        &client,
        day,
        &["GLSV00UKR".to_string()],
        &ALL_SYSTEMS,
        3,
        &output_dir,
    );

    assert_eq!(outcomes.len(), 1);
    let output_path = outcomes[0]
        .result
        .as_ref()
        .unwrap_or_else(|err| panic!("GLSV00UKR should decompress successfully now: {err}"))
        .clone();

    let written = rinex::prelude::Rinex::from_file(&output_path)
        .expect("written output should itself be valid RINEX");
    assert!(
        written.record.as_obs().is_some_and(|obs| !obs.is_empty()),
        "filtered obs record should not be empty for --systems all"
    );
}

#[test]
#[ignore = "hits the live CDDIS archive; needs RINEXFETCH_TEST_TOKEN"]
fn real_obs_unknown_station_is_isolated_from_others() {
    let token = std::env::var("RINEXFETCH_TEST_TOKEN")
        .expect("set RINEXFETCH_TEST_TOKEN to a real URS bearer token to run this test");
    let client = CddisClient::new(token).unwrap();

    let day = GpsDay::resolve("2026-08-01").unwrap();
    let output_dir = TestOutputDir::new("rinexfetch-live-test-obs-isolation");

    // A well-formed but bogus 9-character station ID mixed with a real
    // one: the bogus one should fail in isolation (404 from CDDIS -> not
    // found) without preventing the real station from succeeding.
    let outcomes = obs::fetch_and_write_all(
        &client,
        day,
        &["ZZZZ00ZZZ".to_string(), "WTZR00DEU".to_string()],
        &ALL_SYSTEMS,
        4,
        &output_dir,
    );

    assert_eq!(outcomes.len(), 2);
    assert!(
        matches!(outcomes[0].result, Err(obs::ObsError::NotFound)),
        "bogus station should fail with NotFound, got {:?}",
        outcomes[0].result
    );
    assert!(
        outcomes[1].result.is_ok(),
        "real station should still succeed despite the other one failing: {:?}",
        outcomes[1].result
    );
}

#[test]
#[ignore = "hits the live CDDIS archive; needs RINEXFETCH_TEST_TOKEN"]
fn real_obs_product_at_rinex3() {
    let token = std::env::var("RINEXFETCH_TEST_TOKEN")
        .expect("set RINEXFETCH_TEST_TOKEN to a real URS bearer token to run this test");
    let client = CddisClient::new(token).unwrap();

    let day = GpsDay::resolve("2026-08-01").unwrap();
    let output_dir = TestOutputDir::new("rinexfetch-live-test-obs-v3");

    let outcomes = obs::fetch_and_write_all(
        &client,
        day,
        &["WTZR00DEU".to_string()],
        &ALL_SYSTEMS,
        3,
        &output_dir,
    );

    assert_eq!(outcomes.len(), 1);
    // Plan §12 flagged obs version-conversion as an edge case needing
    // validation against real station data; unlike nav's 4->3
    // downconversion, there's no known domain reason for obs conversion
    // to fail, so this asserts success rather than merely observing
    // whatever happens.
    match &outcomes[0].result {
        Ok(path) => {
            let written = rinex::prelude::Rinex::from_file(path)
                .expect("written output should itself be valid RINEX");
            assert_eq!(written.header.version.major, 3);
        }
        Err(err) => panic!("--rinex-version 3 obs conversion failed: {err}"),
    }
}

#[test]
#[ignore = "hits the live CDDIS archive; needs RINEXFETCH_TEST_TOKEN"]
fn real_latest_nav_survives_a_malformed_candidate() {
    let token = std::env::var("RINEXFETCH_TEST_TOKEN")
        .expect("set RINEXFETCH_TEST_TOKEN to a real URS bearer token to run this test");
    let client = CddisClient::new(token).unwrap();

    // Mirrors --time latest against whatever CDDIS actually has published
    // right now. Regression test for a real bug (2026-08-13): CDDIS's own
    // merge tooling can produce a malformed combined nav file for a given
    // day/tier (a missing newline between two concatenated per-source
    // RINEX headers was observed on 2026 day 224's final tier).
    // fetch_and_write must fall through to the next candidate instead of
    // aborting the whole run over one bad candidate.
    let anchor = GpsDay::resolve("latest").unwrap();
    let candidates = discovery::nav_candidates_for_latest(anchor);

    let output_dir = TestOutputDir::new("rinexfetch-live-test-nav-latest");

    let outcome = nav::fetch_and_write(&client, &candidates, &ALL_SYSTEMS, 4, &output_dir)
        .expect("--time latest should succeed even if some candidate along the way is malformed");

    assert!(outcome.output_path.exists());

    // Re-parsing our own output is a bonus check, not the primary point of
    // this test (already proven above: fetch_and_write didn't abort). It
    // can hit a separate, already-documented rinex crate bug: certain
    // Klobuchar ionosphere model content that rinex itself writes can then
    // panic rinex's own parser on re-read (KbModel::parse bounds panic,
    // observed 2026-08-13) — a round-trip issue in the crate, not
    // something rinexfetch's production code ever triggers, since it
    // never re-parses its own output. Tolerate that specific panic here
    // rather than letting it crash the whole test binary.
    let reparsed =
        std::panic::catch_unwind(|| rinex::prelude::Rinex::from_file(&outcome.output_path));
    match reparsed {
        Ok(Ok(written)) => {
            assert!(
                written.record.as_nav().is_some_and(|nav| !nav.is_empty()),
                "filtered nav record should not be empty for --systems all"
            );
        }
        Ok(Err(err)) => panic!("written output should itself be valid RINEX: {err}"),
        Err(_) => eprintln!(
            "note: re-parsing rinexfetch's own output panicked inside the rinex crate \
             (known Klobuchar round-trip bug) — not asserting on it further"
        ),
    }
}
