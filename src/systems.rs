//! GNSS constellation selection, shared by CLI parsing (`--systems`) and,
//! from Phase 4 onward, by nav/obs system filtering.

use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GnssSystem {
    Gps,
    Glonass,
    Galileo,
    Beidou,
    Qzss,
    Sbas,
}

impl fmt::Display for GnssSystem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            GnssSystem::Gps => "gps",
            GnssSystem::Glonass => "glonass",
            GnssSystem::Galileo => "galileo",
            GnssSystem::Beidou => "beidou",
            GnssSystem::Qzss => "qzss",
            GnssSystem::Sbas => "sbas",
        };
        f.write_str(name)
    }
}

impl FromStr for GnssSystem {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "gps" => Ok(GnssSystem::Gps),
            "glonass" => Ok(GnssSystem::Glonass),
            "galileo" => Ok(GnssSystem::Galileo),
            "beidou" => Ok(GnssSystem::Beidou),
            "qzss" => Ok(GnssSystem::Qzss),
            "sbas" => Ok(GnssSystem::Sbas),
            other => Err(format!(
                "unknown GNSS system {other:?} (expected one of: all, gps, glonass, galileo, beidou, qzss, sbas)"
            )),
        }
    }
}

pub const ALL_SYSTEMS: [GnssSystem; 6] = [
    GnssSystem::Gps,
    GnssSystem::Glonass,
    GnssSystem::Galileo,
    GnssSystem::Beidou,
    GnssSystem::Qzss,
    GnssSystem::Sbas,
];

/// Parses a `--systems` value: either `all`, or a comma-separated subset of
/// `gps,glonass,galileo,beidou,qzss,sbas`.
pub fn parse_systems(raw: &str) -> Result<Vec<GnssSystem>, String> {
    if raw.trim().eq_ignore_ascii_case("all") {
        return Ok(ALL_SYSTEMS.to_vec());
    }

    raw.split(',').map(GnssSystem::from_str).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_expands_to_every_system() {
        assert_eq!(parse_systems("all").unwrap(), ALL_SYSTEMS.to_vec());
        assert_eq!(parse_systems("ALL").unwrap(), ALL_SYSTEMS.to_vec());
    }

    #[test]
    fn parses_comma_separated_subset() {
        assert_eq!(
            parse_systems("gps,galileo").unwrap(),
            vec![GnssSystem::Gps, GnssSystem::Galileo]
        );
    }

    #[test]
    fn rejects_unknown_system() {
        assert!(parse_systems("gps,starlink").is_err());
    }
}
