//! Retrying, resumable, checksum-verified downloads. Validates that
//! retrieved content is actually gzip/RINEX before treating a request as
//! successful, since a failed Earthdata login returns an HTML login page
//! rather than an HTTP error status.
//!
//! Phase 2-3 of the project plan. Not yet implemented.
