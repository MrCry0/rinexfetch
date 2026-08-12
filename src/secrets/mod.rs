//! Credential backends behind the `CredentialProvider` trait (plan §6).

mod provider;

pub mod interactive;

/// OS-native keyring backend (Linux Secret Service, macOS Keychain,
/// Windows Credential Manager), with save-on-first-successful-auth.
/// Phase 2 of the project plan.
pub mod keyring;

pub use provider::{CredentialError, CredentialProvider};
