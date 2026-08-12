//! The `CredentialProvider` abstraction (project plan §6).
//!
//! CDDIS accepts a NASA Earthdata Login (URS) bearer token directly via the
//! `Authorization` header: confirmed against the live archive, an
//! unauthenticated `GET` on a protected file gets a `302` to
//! `urs.earthdata.nasa.gov/oauth/authorize`, while the same request with
//! `Authorization: Bearer <token>` is served the file directly. That makes a
//! token the whole credential for v1 (no username/password or session-cookie
//! exchange needed). CDDIS auth logic (Phase 2) is written against this
//! trait rather than any specific backend, so new backends (OS keyring, then
//! later Vault, AWS Secrets Manager, etc.) can be added without touching it.

#[derive(Debug, thiserror::Error)]
pub enum CredentialError {
    #[error("failed to read credentials: {0}")]
    Io(#[from] std::io::Error),
}

pub trait CredentialProvider {
    /// Returns the Earthdata Login (URS) bearer token to authenticate with.
    fn token(&self) -> Result<String, CredentialError>;
}
