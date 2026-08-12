//! Structured, top-level error type for `rinexfetch`.
//!
//! Per the project plan (§8 Reliability Considerations), failures must be
//! classified rather than surfaced as opaque errors, so lab operators can
//! tell an auth failure apart from a not-yet-published product, a network
//! error, an unknown station, or a parse/format problem. Module-specific
//! error types convert into this one via `#[from]` as each phase lands.

use crate::cddis::auth::CddisAuthError;
use crate::secrets::CredentialError;
use crate::time::TimeError;

#[derive(Debug, thiserror::Error)]
pub enum RinexFetchError {
    #[error("invalid --systems value: {0}")]
    InvalidSystems(String),

    #[error("unsupported --rinex-version {0}: only RINEX 4 output is supported")]
    UnsupportedRinexVersion(u8),

    #[error(transparent)]
    Time(#[from] TimeError),

    #[error(transparent)]
    Credential(#[from] CredentialError),

    #[error(transparent)]
    Auth(#[from] CddisAuthError),
}
