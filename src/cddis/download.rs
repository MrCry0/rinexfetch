//! Downloads a CDDIS product and validates that the response is actually
//! gzip/RINEX data before treating it as successful, as defense in depth:
//! the primary auth-failure signal is the redirect `CddisClient` already
//! detects (plan §5, §8); this guards against any other unexpected
//! response shape. Retrying and resuming partial downloads is Phase 5.

use reqwest::header::CONTENT_TYPE;

use super::auth::{CddisAuthError, CddisClient};

const GZIP_MAGIC: [u8; 2] = [0x1f, 0x8b];

#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error(transparent)]
    Auth(#[from] CddisAuthError),
    #[error("request to CDDIS failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("CDDIS returned an HTML response instead of the requested file")]
    UnexpectedHtml,
    #[error("response does not start with the gzip magic bytes")]
    NotGzip,
}

/// Downloads `url` via `client`, returning the raw (still gzip-compressed)
/// bytes once validated.
pub fn download(client: &CddisClient, url: &str) -> Result<Vec<u8>, DownloadError> {
    let response = client.get(url)?.error_for_status()?;

    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_string();
    if content_type.starts_with("text/html") {
        return Err(DownloadError::UnexpectedHtml);
    }

    let bytes = response.bytes()?;
    if !bytes.starts_with(&GZIP_MAGIC) {
        return Err(DownloadError::NotGzip);
    }

    Ok(bytes.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

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
    fn gzip_response_is_returned() {
        let url = respond_once(
            b"HTTP/1.1 200 OK\r\n\
             Content-Type: application/x-gzip\r\n\
             Content-Length: 4\r\n\
             Connection: close\r\n\r\n\
             \x1f\x8b\x08\x00",
        );

        let client = CddisClient::new("test-token".to_string()).unwrap();
        let bytes = download(&client, &url).unwrap();

        assert_eq!(bytes, vec![0x1f, 0x8b, 0x08, 0x00]);
    }

    #[test]
    fn html_response_is_rejected() {
        let url = respond_once(
            b"HTTP/1.1 200 OK\r\n\
             Content-Type: text/html; charset=UTF-8\r\n\
             Content-Length: 13\r\n\
             Connection: close\r\n\r\n\
             <html></html>",
        );

        let client = CddisClient::new("test-token".to_string()).unwrap();
        let err = download(&client, &url).unwrap_err();

        assert!(matches!(err, DownloadError::UnexpectedHtml));
    }

    #[test]
    fn non_gzip_body_is_rejected() {
        let url = respond_once(
            b"HTTP/1.1 200 OK\r\n\
             Content-Type: application/octet-stream\r\n\
             Content-Length: 4\r\n\
             Connection: close\r\n\r\n\
             plai",
        );

        let client = CddisClient::new("test-token".to_string()).unwrap();
        let err = download(&client, &url).unwrap_err();

        assert!(matches!(err, DownloadError::NotGzip));
    }

    #[test]
    fn not_found_is_reported_as_request_error() {
        let url = respond_once(
            b"HTTP/1.1 404 Not Found\r\n\
             Content-Length: 0\r\n\
             Connection: close\r\n\r\n",
        );

        let client = CddisClient::new("test-token".to_string()).unwrap();
        let err = download(&client, &url).unwrap_err();

        assert!(matches!(err, DownloadError::Request(_)));
    }
}
