//! RINEX 3.xx/4.xx parsing, system filtering, version conversion, and
//! writing. Nav and obs are both implemented.

/// Multi-GNSS nav parse, system-filter, convert version, write. Phase 3-4.
pub mod nav;

/// Per-station obs parse, system-filter, convert version, write, with
/// per-station error isolation. Phase 5.
pub mod obs;

use rinex::prelude::{Header, Version};

/// Sets `header`'s RINEX version to `target_major` if it differs from the
/// source's, using the standard minor version this project targets for
/// each major (3.05, matching `BRDC00IGS`; 4.00, matching `BRD400DLR`).
/// Shared by nav and obs so both convert consistently. Converting up
/// (3->4) works; converting down (4->3) may fail for a RINEX-4-native
/// source with `FormattingError::MissingNavigationStandards` — callers
/// handle that themselves, since the two record types report it
/// differently (see `nav::NavError::UnsupportedDownconversion`).
pub(crate) fn convert_version(header: Header, target_major: u8) -> Header {
    if header.version.major == target_major {
        return header;
    }
    let target_minor = if target_major >= 4 { 0 } else { 5 };
    header.with_version(Version::new(target_major, target_minor))
}
