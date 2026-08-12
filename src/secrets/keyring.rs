//! OS-native keyring `CredentialProvider` backend: Linux Secret Service,
//! macOS Keychain, and Windows Credential Manager storage of the Earthdata
//! Login bearer token (via the `keyring` crate), falling back to the
//! interactive backend if no stored token is found.

use super::interactive::InteractiveCredentialProvider;
use super::provider::{CredentialError, CredentialProvider};

const SERVICE: &str = "rinexfetch";
const ENTRY_NAME: &str = "urs-token";

pub struct KeyringCredentialProvider;

impl CredentialProvider for KeyringCredentialProvider {
    fn token(&self) -> Result<String, CredentialError> {
        let entry = keyring::Entry::new(SERVICE, ENTRY_NAME)?;
        match entry.get_password() {
            Ok(token) => Ok(token),
            Err(keyring::Error::NoEntry) => InteractiveCredentialProvider.token(),
            Err(err) => Err(err.into()),
        }
    }

    /// Persists `token` to the OS keyring now that it's confirmed to work
    /// against CDDIS ("save-on-first-successful-auth", plan §6.2). Runs
    /// even when the token came from an already-stored entry, which just
    /// re-writes the same value.
    fn on_verified(&self, token: &str) -> Result<(), CredentialError> {
        let entry = keyring::Entry::new(SERVICE, ENTRY_NAME)?;
        entry.set_password(token)?;
        Ok(())
    }
}
