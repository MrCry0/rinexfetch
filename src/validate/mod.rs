//! Independent validation of the RINEX files this tool writes.
//!
//! The `rinex` crate reading back its own output proves little: a writer
//! and a parser can share a bug (zero fields written as blanks, for
//! instance, round-trips fine). These checks work on the written text
//! itself, against the fixed-width layout from the RINEX specification
//! and, for nav, against the text of the source file.

pub mod nav;
pub mod obs;

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// The output is malformed or does not match its source.
    Error,
    /// Worth reporting, but not necessarily wrong (e.g. a version
    /// conversion that cannot be compared record by record).
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    pub severity: Severity,
    /// Where it was found, e.g. `line 42` or `record G01 2025 04 13 00 00 00`.
    pub location: String,
    pub message: String,
}

impl fmt::Display for Issue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let level = match self.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        write!(f, "{level}: {}: {}", self.location, self.message)
    }
}

/// How many issues of each severity a report prints in full before it
/// summarises the rest. A systematic defect produces one issue per
/// record, and thousands of identical lines help nobody.
const MAX_PRINTED: usize = 10;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Report {
    pub issues: Vec<Issue>,
}

impl Report {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn error(&mut self, location: impl Into<String>, message: impl Into<String>) {
        self.issues.push(Issue {
            severity: Severity::Error,
            location: location.into(),
            message: message.into(),
        });
    }

    pub fn warning(&mut self, location: impl Into<String>, message: impl Into<String>) {
        self.issues.push(Issue {
            severity: Severity::Warning,
            location: location.into(),
            message: message.into(),
        });
    }

    pub fn extend(&mut self, other: Report) {
        self.issues.extend(other.issues);
    }

    pub fn error_count(&self) -> usize {
        self.count(Severity::Error)
    }

    pub fn warning_count(&self) -> usize {
        self.count(Severity::Warning)
    }

    pub fn has_errors(&self) -> bool {
        self.error_count() > 0
    }

    fn count(&self, severity: Severity) -> usize {
        self.issues
            .iter()
            .filter(|issue| issue.severity == severity)
            .count()
    }

    /// Human-readable lines: the first few issues of each severity in
    /// full, then a count of the rest.
    pub fn render(&self) -> Vec<String> {
        let mut lines = Vec::new();
        for severity in [Severity::Error, Severity::Warning] {
            let matching: Vec<&Issue> = self
                .issues
                .iter()
                .filter(|issue| issue.severity == severity)
                .collect();
            lines.extend(matching.iter().take(MAX_PRINTED).map(|i| i.to_string()));
            if matching.len() > MAX_PRINTED {
                let level = match severity {
                    Severity::Error => "error",
                    Severity::Warning => "warning",
                };
                lines.push(format!(
                    "... and {} more {level}(s)",
                    matching.len() - MAX_PRINTED
                ));
            }
        }
        lines
    }
}

/// A RINEX header line's label (columns 61-80), trimmed.
pub(crate) fn header_label(line: &str) -> &str {
    line.get(60..).unwrap_or("").trim()
}

/// Parses a RINEX numeric field: blank is `None`, otherwise a float that
/// may use a Fortran `D` exponent. `Err` for anything else, which in a
/// fixed-width file usually means the columns are misaligned.
pub(crate) fn parse_field(field: &str) -> Result<Option<f64>, ()> {
    let trimmed = field.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    trimmed
        .replace(['D', 'd'], "E")
        .parse::<f64>()
        .map(Some)
        .map_err(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_field_accepts_blank_and_fortran_exponents() {
        assert_eq!(parse_field("    "), Ok(None));
        assert_eq!(parse_field(" 1.5D+02"), Ok(Some(150.0)));
        assert_eq!(parse_field("-2.0e-03"), Ok(Some(-0.002)));
    }

    #[test]
    fn parse_field_rejects_a_trailing_sign_from_a_shifted_column() {
        assert_eq!(parse_field("1.676380634308E-08-"), Err(()));
    }

    #[test]
    fn render_summarises_beyond_the_print_limit() {
        let mut report = Report::new();
        for n in 0..(MAX_PRINTED + 5) {
            report.error(format!("line {n}"), "bad");
        }
        let lines = report.render();
        assert_eq!(lines.len(), MAX_PRINTED + 1);
        assert_eq!(lines.last().unwrap(), "... and 5 more error(s)");
    }
}
