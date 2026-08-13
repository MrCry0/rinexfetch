//! OS-native keyring `CredentialProvider` backend: Linux Secret Service,
//! macOS Keychain, and Windows Credential Manager storage of the Earthdata
//! Login bearer token (via the `keyring` crate), falling back to the
//! interactive backend if no stored token is found. Persisting a freshly
//! entered token requires explicit user consent (see `on_verified`).

use std::io::{self, Write};

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
    /// against CDDIS ("save-on-first-successful-auth", plan §6.2) — but
    /// only for a freshly entered token, and only with explicit consent.
    /// If an entry already exists, `token` is what was already loaded from
    /// it, so there's nothing new to persist or ask about: this returns
    /// immediately rather than prompting on every run.
    fn on_verified(&self, token: &str) -> Result<(), CredentialError> {
        let entry = keyring::Entry::new(SERVICE, ENTRY_NAME)?;
        match entry.get_password() {
            Ok(_) => Ok(()),
            Err(keyring::Error::NoEntry) => {
                if confirm("Save this token to your OS keyring for future runs? [y/N] ")? {
                    entry.set_password(token)?;
                }
                Ok(())
            }
            Err(err) => Err(err.into()),
        }
    }
}

/// Prints `prompt`, reads a line from stdin, and returns whether it was an
/// affirmative answer.
fn confirm(prompt: &str) -> Result<bool, CredentialError> {
    print!("{prompt}");
    io::stdout().flush()?;
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    Ok(is_affirmative(&answer))
}

/// `y`/`yes`, case-insensitive; anything else, including empty input, is
/// "no" — matching the `[y/N]` convention (default is no).
fn is_affirmative(answer: &str) -> bool {
    matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_y_and_yes_case_insensitively() {
        for input in ["y", "Y", "yes", "Yes", "YES", "  y  ", "y\n"] {
            assert!(is_affirmative(input), "expected {input:?} to be yes");
        }
    }

    #[test]
    fn rejects_anything_else_including_empty() {
        for input in ["", "\n", "n", "no", "N", "maybe", "yesplease"] {
            assert!(!is_affirmative(input), "expected {input:?} to be no");
        }
    }
}
