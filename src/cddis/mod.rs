//! CDDIS access: Earthdata Login, product discovery, and download.
//!
//! Auth and download are implemented (Phase 2). Discovery is Phase 3.

/// Earthdata Login (URS) bearer-token auth: attaches the token as an
/// `Authorization` header. Phase 2.
pub mod auth;

/// Resolves remote CDDIS paths for nav & obs products, including the
/// `--time latest` fallback tiers (final / rapid). Phase 3. Not yet
/// implemented.
pub mod discovery;

/// Downloads a CDDIS product and validates the response, with content-type
/// and magic-byte checks as defense in depth against an unexpected
/// non-file response. Retry/resume is Phase 5. Phase 2.
pub mod download;
