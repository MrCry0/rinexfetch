//! Layout checks on a written RINEX 3/4 observation file. The source of
//! an obs file is Hatanaka-compressed, so only the output text is
//! checked, against its own header.

use std::collections::HashMap;

use super::{Report, header_label, parse_field};

/// One observation: `F14.3` value, loss-of-lock and signal-strength flags.
const OBS_WIDTH: usize = 16;
const VALUE_WIDTH: usize = 14;

/// Declared observation-type counts per system, from `SYS / # / OBS TYPES`.
fn declared_types(header: &[&str]) -> HashMap<char, usize> {
    let mut declared = HashMap::new();
    for line in header {
        if header_label(line) != "SYS / # / OBS TYPES" {
            continue;
        }
        let Some(system) = line.chars().next().filter(|c| c.is_ascii_uppercase()) else {
            continue; // continuation of a previous line's type list
        };
        if let Some(count) = line.get(3..6).and_then(|n| n.trim().parse::<usize>().ok()) {
            declared.insert(system, count);
        }
    }
    declared
}

pub fn check_structure(output: &str) -> Report {
    let mut report = Report::new();
    let lines: Vec<&str> = output.lines().collect();

    match lines.first() {
        Some(first) if first.contains("RINEX VERSION / TYPE") => {
            if !first.contains("OBSERVATION DATA") {
                report.error("line 1", "not an observation data file");
            }
        }
        _ => {
            report.error("line 1", "missing RINEX VERSION / TYPE header line");
            return report;
        }
    }

    let Some(end) = lines
        .iter()
        .position(|l| header_label(l) == "END OF HEADER")
    else {
        report.error("header", "missing END OF HEADER");
        return report;
    };
    let declared = declared_types(&lines[..end]);
    if declared.is_empty() {
        report.error("header", "no SYS / # / OBS TYPES lines");
    }

    let mut index = end + 1;
    let mut epochs = 0usize;
    let mut previous: Option<String> = None;
    while index < lines.len() {
        let line = lines[index];
        let number = index + 1;
        index += 1;
        if line.trim().is_empty() {
            continue;
        }
        let Some(rest) = line.strip_prefix('>') else {
            report.error(
                format!("line {number}"),
                "expected an epoch line starting with '>'",
            );
            continue;
        };

        epochs += 1;
        let stamp = rest.get(1..28).unwrap_or("").to_string();
        if let Some(prev) = previous.as_ref().filter(|prev| stamp < **prev) {
            report.error(
                format!("line {number}"),
                format!("epoch {stamp:?} is earlier than the previous {prev:?}"),
            );
        }
        previous = Some(stamp);

        let flag = rest.get(29..31).unwrap_or("").trim().parse::<u8>();
        let count = rest.get(31..34).unwrap_or("").trim().parse::<usize>();
        let (Ok(flag), Ok(count)) = (flag, count) else {
            report.error(
                format!("line {number}"),
                "epoch line has no valid event flag / satellite count",
            );
            continue;
        };

        // Event flags above 1 (and 6) carry header-style lines, not
        // observations; they are skipped, not checked.
        let is_observation_epoch = flag <= 1 || flag == 6;
        if !is_observation_epoch {
            index = (index + count).min(lines.len());
            continue;
        }

        for offset in 0..count {
            let Some(sat_line) = lines.get(index + offset) else {
                report.error(
                    format!("line {number}"),
                    format!(
                        "epoch announces {count} satellite(s) but the file ends after {offset}"
                    ),
                );
                break;
            };
            if sat_line.starts_with('>') {
                report.error(
                    format!("line {number}"),
                    format!("epoch announces {count} satellite(s) but only {offset} follow"),
                );
                break;
            }
            check_satellite_line(sat_line, index + offset + 1, &declared, &mut report);
        }
        index += count;
    }

    if epochs == 0 {
        report.error("body", "no epochs");
    }
    report
}

fn check_satellite_line(
    line: &str,
    number: usize,
    declared: &HashMap<char, usize>,
    report: &mut Report,
) {
    let Some(system) = line.chars().next().filter(|c| c.is_ascii_uppercase()) else {
        report.error(
            format!("line {number}"),
            "satellite line has no system letter",
        );
        return;
    };
    let body = line.get(3..).unwrap_or("");
    let fields = body.len().div_ceil(OBS_WIDTH);

    match declared.get(&system) {
        None => report.error(
            format!("line {number}"),
            format!("system {system} has no SYS / # / OBS TYPES entry"),
        ),
        Some(&count) if fields > count => report.error(
            format!("line {number}"),
            format!("{fields} observation fields for system {system}, header declares {count}"),
        ),
        Some(_) => {}
    }

    for i in 0..fields {
        let start = i * OBS_WIDTH;
        let value = body
            .get(start..(start + VALUE_WIDTH).min(body.len()))
            .unwrap_or("");
        if parse_field(value).is_err() {
            report.error(
                format!("line {number}"),
                format!("observation field {} {value:?} is not a number", i + 1),
            );
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADER: &str =
        "     3.04           OBSERVATION DATA    M: MIXED            RINEX VERSION / TYPE
G    3 C1C L1C S1C                                          SYS / # / OBS TYPES
R    2 C1C L1C                                              SYS / # / OBS TYPES
                                                            END OF HEADER
";

    const EPOCH: &str = "> 2026 10 01 00 00  0.0000000  0  2
G07  25062001.100 7 131701705.800 7        49.250
R01  22966488.228 7 120689720.596 7
";

    #[test]
    fn well_formed_file_passes() {
        let report = check_structure(&format!("{HEADER}{EPOCH}"));
        assert!(!report.has_errors(), "{:?}", report.render());
    }

    #[test]
    fn missing_satellite_lines_are_an_error() {
        let truncated = EPOCH.replace("  2\n", "  3\n");
        let report = check_structure(&format!("{HEADER}{truncated}"));
        assert!(report.has_errors());
    }

    #[test]
    fn more_fields_than_declared_is_an_error() {
        let extra = EPOCH.replace(
            "R01  22966488.228 7 120689720.596 7\n",
            "R01  22966488.228 7 120689720.596 7        49.250\n",
        );
        assert!(check_structure(&format!("{HEADER}{extra}")).has_errors());
    }

    #[test]
    fn undeclared_system_is_an_error() {
        let galileo = EPOCH.replace("R01", "E01");
        assert!(check_structure(&format!("{HEADER}{galileo}")).has_errors());
    }

    #[test]
    fn non_numeric_observation_is_an_error() {
        let bad = EPOCH.replace("25062001.100", "2506200x.100");
        assert!(check_structure(&format!("{HEADER}{bad}")).has_errors());
    }

    #[test]
    fn decreasing_epochs_are_an_error() {
        let later = EPOCH.replace("00  0.0000000", "00 30.0000000");
        let report = check_structure(&format!("{HEADER}{later}{EPOCH}"));
        assert!(report.has_errors());
    }

    #[test]
    fn file_without_epochs_is_an_error() {
        assert!(check_structure(HEADER).has_errors());
    }
}
