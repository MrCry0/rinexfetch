//! Earthdata Login (URS) bearer-token auth: attaches
//! `Authorization: Bearer <token>` to CDDIS requests. No cookie jar or
//! redirect-following login flow is needed — confirmed against the live
//! archive, CDDIS accepts a URS token directly on the file request itself.

use reqwest::StatusCode;
use reqwest::blocking::{Client, Response};
use reqwest::header::{AUTHORIZATION, LOCATION, RANGE};
use reqwest::redirect::Policy;

/// A long-archived, permanently stable CDDIS product used only to confirm
/// a bearer token works, without downloading a whole file (see
/// `verify_token`).
const TOKEN_VERIFICATION_URL: &str = "https://cddis.nasa.gov/archive/gnss/data/daily/2020/001/20p/BRDC00IGS_R_20200010000_01D_MN.rnx.gz";

#[derive(Debug, thiserror::Error)]
pub enum CddisAuthError {
    /// No `Authorization` header (or a header CDDIS's proxy doesn't even
    /// look past) gets a redirect back to the Earthdata Login page instead
    /// of the file.
    #[error("CDDIS rejected the bearer token (redirected to Earthdata Login: {location})")]
    Unauthenticated { location: String },
    /// A present but invalid/expired token gets a direct `401`/`403` from
    /// CDDIS, not a redirect. Confirmed against the live archive: a
    /// syntactically-present garbage token gets `401 Unauthorized`.
    #[error("CDDIS rejected the bearer token (HTTP {status})")]
    InvalidToken { status: StatusCode },
    #[error("request to CDDIS failed: {0}")]
    Request(#[from] reqwest::Error),
}

/// HTTP client that attaches a URS bearer token to every CDDIS request and
/// treats a redirect back to Earthdata Login as the auth failure it is,
/// rather than following it (plan §5).
pub struct CddisClient {
    http: Client,
    token: String,
}

impl CddisClient {
    pub fn new(token: String) -> Result<Self, CddisAuthError> {
        let http = Client::builder().redirect(Policy::none()).build()?;
        Ok(Self { http, token })
    }

    /// Issues an authenticated GET against a CDDIS URL.
    pub fn get(&self, url: &str) -> Result<Response, CddisAuthError> {
        self.send(url, None)
    }

    /// Confirms the token works by requesting a single byte of a stable,
    /// long-archived product, rather than downloading a whole file. Used
    /// to verify a freshly entered token before it's persisted (see
    /// `secrets::keyring`) and to fail fast with a clear error on startup
    /// rather than only discovering an auth problem during Phase 3/4
    /// product discovery and download.
    pub fn verify_token(&self) -> Result<(), CddisAuthError> {
        self.send(TOKEN_VERIFICATION_URL, Some("bytes=0-0"))?;
        Ok(())
    }

    fn send(&self, url: &str, range: Option<&str>) -> Result<Response, CddisAuthError> {
        let mut request = self
            .http
            .get(url)
            .header(AUTHORIZATION, format!("Bearer {}", self.token));
        if let Some(range) = range {
            request = request.header(RANGE, range);
        }
        let response = request.send()?;
        let status = response.status();

        if status.is_redirection() {
            let location = response
                .headers()
                .get(LOCATION)
                .and_then(|value| value.to_str().ok())
                .unwrap_or("")
                .to_string();
            if location.contains("urs.earthdata.nasa.gov") {
                return Err(CddisAuthError::Unauthenticated { location });
            }
        }

        if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
            return Err(CddisAuthError::InvalidToken { status });
        }

        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    /// Starts a one-shot local HTTP server that replies with `response` to
    /// its single connection, and returns the URL to hit. Keeps the auth
    /// classification logic testable against real HTTP semantics without
    /// pulling in a mocking crate or touching the network.
    fn respond_once(response: &'static [u8]) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 4096];
                let _ = stream.read(&mut buf);
                let _ = stream.write_all(response);
            }
        });
        format!("http://{addr}/probe")
    }

    #[test]
    fn redirect_to_urs_is_reported_as_unauthenticated() {
        let url = respond_once(
            b"HTTP/1.1 302 Found\r\n\
             Location: https://urs.earthdata.nasa.gov/oauth/authorize?client_id=x\r\n\
             Content-Length: 0\r\n\
             Connection: close\r\n\r\n",
        );

        let client = CddisClient::new("test-token".to_string()).unwrap();
        let err = client.get(&url).unwrap_err();

        assert!(matches!(err, CddisAuthError::Unauthenticated { location }
            if location.contains("urs.earthdata.nasa.gov")));
    }

    #[test]
    fn success_response_is_returned() {
        let url = respond_once(
            b"HTTP/1.1 200 OK\r\n\
             Content-Type: application/x-gzip\r\n\
             Content-Length: 4\r\n\
             Connection: close\r\n\r\n\
             \x1f\x8b\x08\x00",
        );

        let client = CddisClient::new("test-token".to_string()).unwrap();
        let response = client.get(&url).unwrap();

        assert_eq!(response.status(), reqwest::StatusCode::OK);
    }

    #[test]
    fn non_urs_redirect_is_passed_through() {
        let url = respond_once(
            b"HTTP/1.1 302 Found\r\n\
             Location: https://cddis.nasa.gov/elsewhere\r\n\
             Content-Length: 0\r\n\
             Connection: close\r\n\r\n",
        );

        let client = CddisClient::new("test-token".to_string()).unwrap();
        let response = client.get(&url).unwrap();

        assert_eq!(response.status(), reqwest::StatusCode::FOUND);
    }

    #[test]
    fn invalid_token_401_is_reported_as_invalid_token() {
        let url = respond_once(
            b"HTTP/1.1 401 Unauthorized\r\n\
             Content-Length: 0\r\n\
             Connection: close\r\n\r\n",
        );

        let client = CddisClient::new("garbage-token".to_string()).unwrap();
        let err = client.get(&url).unwrap_err();

        assert!(matches!(
            err,
            CddisAuthError::InvalidToken {
                status: StatusCode::UNAUTHORIZED
            }
        ));
    }
}
