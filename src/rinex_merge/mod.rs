//! RINEX 4.xx parsing, system filtering, merging, and writing.
//!
//! Nav is implemented (Phase 3). Obs is Phase 4.

/// Multi-GNSS nav parse, system-filter, upconvert, write. Phase 3.
pub mod nav;

/// Per-station obs parse, system-filter, write. Phase 4. Not yet
/// implemented.
pub mod obs;
