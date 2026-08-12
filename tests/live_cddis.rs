//! Live-network sanity checks against the real CDDIS archive. Not run in CI
//! (network-dependent; `cddis::auth`'s unit tests cover the classification
//! logic hermetically against a local server). Run manually with
//! `cargo test -- --ignored`; the full-pipeline test additionally needs a
//! real bearer token in `RINEXFETCH_TEST_TOKEN`.

use std::fs;

use rinexfetch::cddis::auth::{CddisAuthError, CddisClient};
use rinexfetch::cddis::discovery;
use rinexfetch::rinex_merge::nav;
use rinexfetch::systems::ALL_SYSTEMS;
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

    let outcome = nav::fetch_and_write(&client, &candidates, &ALL_SYSTEMS, &output_dir)
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
