//! CDDIS access: Earthdata Login, product discovery, and download.
//!
//! Phases 2-3 of the project plan. Not yet implemented.

/// Earthdata Login (URS) bearer-token auth: attaches the token as an
/// `Authorization` header. Phase 2.
pub mod auth;

/// Resolves remote CDDIS paths for nav & obs products, including the
/// `--time now` fallback tiers (final / rapid / ultra-rapid). Phase 3.
pub mod discovery;

/// Retrying, resumable, checksum-verified downloads, with content-type
/// validation as defense in depth against an unexpected non-file response.
/// Phase 2-3.
pub mod download;
