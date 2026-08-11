//! CDDIS access: Earthdata Login, product discovery, and download.
//!
//! Phases 2-3 of the project plan. Not yet implemented.

/// Earthdata Login flow and cookie-jar redirect handling. Phase 2.
pub mod auth;

/// Resolves remote CDDIS paths for nav & obs products, including the
/// `--time now` fallback tiers (final / rapid / ultra-rapid). Phase 3.
pub mod discovery;

/// Retrying, resumable, checksum-verified downloads, with content-type
/// validation to detect a login page served in place of the requested
/// file. Phase 2-3.
pub mod download;
