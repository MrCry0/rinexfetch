//! The `CredentialProvider` abstraction (project plan §6).
//!
//! CDDIS auth logic (Phase 2) is written against this trait rather than any
//! specific backend, so new backends (OS keyring, then later Vault, AWS
//! Secrets Manager, etc.) can be added without touching it.

/// NASA Earthdata Login (URS) credentials.
pub struct Credentials {
    pub username: String,
    pub password: String,
}

#[derive(Debug, thiserror::Error)]
pub enum CredentialError {
    #[error("failed to read credentials: {0}")]
    Io(#[from] std::io::Error),
}

pub trait CredentialProvider {
    /// Returns the Earthdata Login credentials to authenticate with.
    fn credentials(&self) -> Result<Credentials, CredentialError>;
}
