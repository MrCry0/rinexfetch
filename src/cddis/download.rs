//! Retrying, resumable, checksum-verified downloads. A missing or invalid
//! bearer token gets a `302` redirect to `urs.earthdata.nasa.gov` rather
//! than a served file, which is easy to detect on status/`Location` alone;
//! this module also validates that retrieved content is actually
//! gzip/RINEX before treating a request as successful, as defense in depth
//! against any other unexpected response shape.
//!
//! Phase 2-3 of the project plan. Not yet implemented.
