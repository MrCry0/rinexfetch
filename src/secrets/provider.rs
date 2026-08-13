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
    #[error(
        "no OS-native credential store is available on this system (Secret Service on Linux, \
         Keychain on macOS, Credential Manager on Windows). On Linux this usually means no \
         D-Bus Secret Service is running, which is common on headless/server systems — use \
         --credential-provider interactive instead, or install and start a Secret Service \
         provider such as gnome-keyring or kwallet."
    )]
    NoKeyringBackend,
    #[error("keyring error: {0}")]
    Keyring(keyring::Error),
}

impl From<keyring::Error> for CredentialError {
    fn from(err: keyring::Error) -> Self {
        match err {
            keyring::Error::NoDefaultStore => CredentialError::NoKeyringBackend,
            other => CredentialError::Keyring(other),
        }
    }
}

pub trait CredentialProvider {
    /// Returns the Earthdata Login (URS) bearer token to authenticate with.
    fn token(&self) -> Result<String, CredentialError>;

    /// Called after `token`'s return value has been confirmed to work
    /// against CDDIS. Backends that persist tokens (the keyring backend)
    /// override this to save on first successful auth; the default is a
    /// no-op.
    fn on_verified(&self, _token: &str) -> Result<(), CredentialError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_default_store_maps_to_a_specific_actionable_error() {
        let err: CredentialError = keyring::Error::NoDefaultStore.into();
        assert!(matches!(err, CredentialError::NoKeyringBackend));
    }

    #[test]
    fn other_keyring_errors_pass_through() {
        let err: CredentialError = keyring::Error::NoEntry.into();
        assert!(matches!(
            err,
            CredentialError::Keyring(keyring::Error::NoEntry)
        ));
    }
}
