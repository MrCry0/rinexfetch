//! Live-network sanity checks against the real CDDIS archive. Not run in CI
//! (network-dependent; `cddis::auth`'s unit tests cover the classification
//! logic hermetically against a local server). Run manually with
//! `cargo test -- --ignored`; the full-pipeline test additionally needs a
//! real bearer token in `RINEXFETCH_TEST_TOKEN`.

use std::fs;

use rinexfetch::cddis::auth::{CddisAuthError, CddisClient};
use rinexfetch::cddis::discovery::{self, NavTier};
use rinexfetch::rinex_merge::nav::{self, NavError};
use rinexfetch::rinex_merge::obs;
use rinexfetch::systems::{ALL_SYSTEMS, GnssSystem};
use rinexfetch::time::GpsDay;

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

    let output_dir = std::env::temp_dir().join("rinexfetch-live-test-nav");
    fs::create_dir_all(&output_dir).unwrap();

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

    fs::remove_dir_all(&output_dir).ok();
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

    let output_dir = std::env::temp_dir().join("rinexfetch-live-test-nav-v3-passthrough");
    fs::create_dir_all(&output_dir).unwrap();

    let outcome = nav::fetch_and_write(&client, &candidates, &ALL_SYSTEMS, 3, &output_dir)
        .expect("same-version (3 -> 3) passthrough should always succeed");

    let written = rinex::prelude::Rinex::from_file(&outcome.output_path)
        .expect("written output should itself be valid RINEX");
    assert_eq!(written.header.version.major, 3);

    fs::remove_dir_all(&output_dir).ok();
}

#[test]
#[ignore = "hits the live CDDIS archive; needs RINEXFETCH_TEST_TOKEN"]
fn real_rapid_nav_product_cannot_downconvert_to_rinex3() {
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

    let output_dir = std::env::temp_dir().join("rinexfetch-live-test-nav-v3-downconvert");
    fs::create_dir_all(&output_dir).unwrap();

    // Confirmed against the live archive: the rinex crate cannot represent
    // a RINEX-4-tagged nav message (BRD400DLR explicitly tags a
    // NavMessageType per record) back in RINEX 3, for any constellation
    // tried (all systems and GPS-only both hit the same limitation). This
    // is a real domain/crate limitation, not a bug in rinexfetch: v1
    // detects it via NavError::UnsupportedDownconversion rather than
    // silently writing a broken or incomplete file.
    let outcome = nav::fetch_and_write(&client, &candidates, &[GnssSystem::Gps], 3, &output_dir);
    assert!(matches!(outcome, Err(NavError::UnsupportedDownconversion)));

    fs::remove_dir_all(&output_dir).ok();
}

#[test]
#[ignore = "hits the live CDDIS archive; needs RINEXFETCH_TEST_TOKEN"]
fn real_obs_product_downloads_and_writes_for_known_station() {
    let token = std::env::var("RINEXFETCH_TEST_TOKEN")
        .expect("set RINEXFETCH_TEST_TOKEN to a real URS bearer token to run this test");
    let client = CddisClient::new(token).unwrap();

    let day = GpsDay::resolve("2026-08-01").unwrap();
    let output_dir = std::env::temp_dir().join("rinexfetch-live-test-obs");
    fs::create_dir_all(&output_dir).unwrap();

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

    fs::remove_dir_all(&output_dir).ok();
}

#[test]
#[ignore = "hits the live CDDIS archive; needs RINEXFETCH_TEST_TOKEN"]
fn real_obs_unknown_station_is_isolated_from_others() {
    let token = std::env::var("RINEXFETCH_TEST_TOKEN")
        .expect("set RINEXFETCH_TEST_TOKEN to a real URS bearer token to run this test");
    let client = CddisClient::new(token).unwrap();

    let day = GpsDay::resolve("2026-08-01").unwrap();
    let output_dir = std::env::temp_dir().join("rinexfetch-live-test-obs-isolation");
    fs::create_dir_all(&output_dir).unwrap();

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

    fs::remove_dir_all(&output_dir).ok();
}

#[test]
#[ignore = "hits the live CDDIS archive; needs RINEXFETCH_TEST_TOKEN"]
fn real_obs_product_at_rinex3() {
    let token = std::env::var("RINEXFETCH_TEST_TOKEN")
        .expect("set RINEXFETCH_TEST_TOKEN to a real URS bearer token to run this test");
    let client = CddisClient::new(token).unwrap();

    let day = GpsDay::resolve("2026-08-01").unwrap();
    let output_dir = std::env::temp_dir().join("rinexfetch-live-test-obs-v3");
    fs::create_dir_all(&output_dir).unwrap();

    let outcomes = obs::fetch_and_write_all(
        &client,
        day,
        &["WTZR00DEU".to_string()],
        &ALL_SYSTEMS,
        3,
        &output_dir,
    );

    assert_eq!(outcomes.len(), 1);
    // Plan §12 flags obs version-conversion edge cases as needing
    // validation against real station data; this documents the actual
    // outcome rather than assuming it works.
    match &outcomes[0].result {
        Ok(path) => {
            let written = rinex::prelude::Rinex::from_file(path)
                .expect("written output should itself be valid RINEX");
            assert_eq!(written.header.version.major, 3);
        }
        Err(err) => panic!("--rinex-version 3 obs conversion failed: {err}"),
    }

    fs::remove_dir_all(&output_dir).ok();
}
