//! Interactive `CredentialProvider` backend: prompts for a NASA Earthdata
//! Login (URS) bearer token (hidden) at runtime. Generate one at
//! `https://urs.earthdata.nasa.gov/users/<username>/user_tokens` (valid 60
//! days, max 2 active at a time). No token is cached or stored anywhere by
//! this backend.

use super::provider::{CredentialError, CredentialProvider};

pub struct InteractiveCredentialProvider;

impl CredentialProvider for InteractiveCredentialProvider {
    fn token(&self) -> Result<String, CredentialError> {
        let token = rpassword::prompt_password("NASA Earthdata Login (URS) bearer token: ")?;
        Ok(token.trim().to_string())
    }
}
