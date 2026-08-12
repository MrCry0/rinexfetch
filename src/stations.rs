//! Station ID validation for `--stations` (plan §4.3).

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StationIdError {
    #[error(
        "station ID {0:?} is a legacy 4-character site code; supply the full \
         9-character IGS station ID instead (e.g. WTZR00DEU) — automatic \
         4-to-9 expansion isn't implemented yet (plan §12)"
    )]
    LegacyFourCharacterId(String),
    #[error(
        "station ID {0:?} is not a valid 9-character IGS station ID \
         (expected XXXX##CCC: 4-char site code, 2-digit monument number, \
         3-letter country code)"
    )]
    InvalidFormat(String),
}

/// Validates and normalizes (uppercases) a single station ID. Accepts only
/// the modern 9-character IGS form (`XXXX##CCC`). 4-character legacy IDs
/// are rejected with a specific, actionable error rather than silently
/// guessing an expansion to 9 characters — the 3-letter country-code
/// suffix isn't derivable from the site code alone without an external
/// station database, which is deferred past v1 (plan §12).
pub fn validate_station_id(raw: &str) -> Result<String, StationIdError> {
    let id = raw.trim().to_ascii_uppercase();

    if id.len() == 4 && id.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err(StationIdError::LegacyFourCharacterId(raw.to_string()));
    }

    let bytes = id.as_bytes();
    let is_valid = id.len() == 9
        && bytes[0..4].iter().all(|b| b.is_ascii_alphanumeric())
        && bytes[4..6].iter().all(|b| b.is_ascii_digit())
        && bytes[6..9].iter().all(|b| b.is_ascii_alphabetic());

    if !is_valid {
        return Err(StationIdError::InvalidFormat(raw.to_string()));
    }

    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_well_formed_nine_character_id() {
        assert_eq!(validate_station_id("WTZR00DEU").unwrap(), "WTZR00DEU");
    }

    #[test]
    fn normalizes_to_uppercase() {
        assert_eq!(validate_station_id("wtzr00deu").unwrap(), "WTZR00DEU");
    }

    #[test]
    fn rejects_legacy_four_character_id() {
        assert_eq!(
            validate_station_id("WTZR").unwrap_err(),
            StationIdError::LegacyFourCharacterId("WTZR".to_string())
        );
    }

    #[test]
    fn rejects_wrong_length() {
        assert!(matches!(
            validate_station_id("WTZR00DE"),
            Err(StationIdError::InvalidFormat(_))
        ));
    }

    #[test]
    fn rejects_non_digit_monument_number() {
        assert!(matches!(
            validate_station_id("WTZRXXDEU"),
            Err(StationIdError::InvalidFormat(_))
        ));
    }

    #[test]
    fn rejects_non_alphabetic_country_code() {
        assert!(matches!(
            validate_station_id("WTZR0012U"),
            Err(StationIdError::InvalidFormat(_))
        ));
    }
}
