//! Interactive `CredentialProvider` backend: prompts for a NASA Earthdata
//! Login username and password (hidden) at runtime. No credentials are
//! cached or stored anywhere by this backend.

use std::io::{self, Write};

use super::provider::{CredentialError, CredentialProvider, Credentials};

pub struct InteractiveCredentialProvider;

impl CredentialProvider for InteractiveCredentialProvider {
    fn credentials(&self) -> Result<Credentials, CredentialError> {
        print!("NASA Earthdata Login username: ");
        io::stdout().flush()?;
        let mut username = String::new();
        io::stdin().read_line(&mut username)?;

        let password = rpassword::prompt_password("NASA Earthdata Login password: ")?;

        Ok(Credentials {
            username: username.trim().to_string(),
            password,
        })
    }
}
