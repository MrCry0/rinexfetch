//! Live-network sanity check against the real CDDIS archive. Not run in CI
//! (network-dependent; `cddis::auth`'s unit tests cover the classification
//! logic hermetically against a local server). Run manually with
//! `cargo test -- --ignored`.

use rinexfetch::cddis::auth::{CddisAuthError, CddisClient};

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
