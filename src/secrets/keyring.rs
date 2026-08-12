//! OS-native keyring `CredentialProvider` backend.
//!
//! Phase 2 of the project plan: Linux Secret Service, macOS Keychain, and
//! Windows Credential Manager storage of the Earthdata Login bearer token,
//! falling back to the interactive backend (with optional save-to-keyring)
//! if no stored token is found. Not yet implemented.
