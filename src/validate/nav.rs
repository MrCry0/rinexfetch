//! Checks on a written navigation file: its fixed-width layout, and (when
//! the source text is available) that every record of the requested
//! systems made it across with the same values.

use std::collections::{HashMap, HashSet};

use super::{Report, header_label, parse_field};
use crate::systems::{ALL_SYSTEMS, GnssSystem};

/// Width of one orbit field (`D19.12`, sign column included).
const FIELD_WIDTH: usize = 19;
/// Orbit values on a continuation line, after the 4-column indent.
const FIELDS_PER_ROW: usize = 4;
const MAX_LINE: usize = 80;

#[derive(Debug)]
struct Record {
    /// `EPH`/`STO`/`ION`/`EOP` for RINEX 4, empty for RINEX 3.
    kind: String,
    sv: String,
    msg: String,
    epoch: String,
    line: usize,
    /// Orbit fields of an ephemeris record: three on the first line, four
    /// on each continuation line. `None` is a blank field.
    fields: Vec<Option<f64>>,
    rows: usize,
    /// Non-ephemeris RINEX 4 records are free-form text; kept normalised.
    text: Option<String>,
}

impl Record {
    fn id(&self) -> String {
        if self.epoch.is_empty() {
            format!("record {} {} {}", self.kind, self.sv, self.msg)
        } else {
            format!("record {} {}", self.sv, self.epoch)
        }
    }

    /// Matching key. Values are not part of it, so a changed value is
    /// reported as a difference rather than as a missing record.
    /// System letter plus message type: records laid out the same way.
    fn group(&self) -> String {
        format!("{}{}", self.sv.chars().next().unwrap_or('?'), self.msg)
    }

    fn key(&self) -> String {
        format!("{}|{}|{}|{}", self.kind, self.sv, self.msg, self.epoch)
    }
}

#[derive(Debug)]
struct Parsed {
    major: u8,
    /// One `<4-letter code> <label>` entry per correction header line.
    corr: Vec<String>,
    records: Vec<Record>,
}

fn slice(line: &str, start: usize, end: usize) -> &str {
    let end = end.min(line.len());
    if start >= end {
        return "";
    }
    line.get(start..end).unwrap_or("")
}

/// A line that starts an ephemeris record: a system letter and two digits.
fn starts_record(line: &str) -> bool {
    let b = line.as_bytes();
    b.len() >= 3 && b[0].is_ascii_uppercase() && b[1].is_ascii_digit() && b[2].is_ascii_digit()
}

fn normalise(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_uppercase()
}

fn parse(text: &str, report: &mut Report) -> Parsed {
    let mut parsed = Parsed {
        major: 0,
        corr: Vec::new(),
        records: Vec::new(),
    };
    let all: Vec<(usize, &str)> = text.lines().enumerate().map(|(i, l)| (i + 1, l)).collect();
    // Blank lines after the last record are not rows of it.
    let end = all
        .iter()
        .rposition(|(_, l)| !l.trim().is_empty())
        .map_or(0, |i| i + 1);
    let mut lines = all[..end].iter().copied();

    match text.lines().next() {
        Some(first) if first.contains("RINEX VERSION / TYPE") => {
            parsed.major = slice(first, 0, 9)
                .trim()
                .parse::<f64>()
                .map(|v| v as u8)
                .unwrap_or(0);
            if !first.contains("NAVIGATION DATA") {
                report.error("line 1", "not a navigation data file");
            }
        }
        _ => report.error("line 1", "missing RINEX VERSION / TYPE header line"),
    }

    let mut header_done = false;
    for (_, line) in lines.by_ref() {
        let label = header_label(line);
        if label.ends_with("CORR") {
            parsed
                .corr
                .push(format!("{} {label}", slice(line, 0, 4).trim()));
        }
        if label == "END OF HEADER" {
            header_done = true;
            break;
        }
    }
    if !header_done {
        report.error("header", "missing END OF HEADER");
        return parsed;
    }

    // Marker of the RINEX 4 record being read: (kind, sv, msg).
    let mut marker: Option<(String, String, String)> = None;

    for (n, line) in lines {
        if line.len() > MAX_LINE {
            report.error(
                format!("line {n}"),
                format!("line is {} columns, the limit is {MAX_LINE}", line.len()),
            );
        }
        if let Some(rest) = line.strip_prefix('>') {
            let mut tokens = rest.split_whitespace();
            let kind = tokens.next().unwrap_or("").to_string();
            let sv = tokens.next().unwrap_or("").to_string();
            // message type, plus the RINEX 4.02 subtype (WIDE, JAPN, ...) if any
            let msg = tokens.collect::<Vec<_>>().join(" ");
            if kind != "EPH" {
                parsed.records.push(Record {
                    kind: kind.clone(),
                    sv: sv.clone(),
                    msg: msg.clone(),
                    epoch: String::new(),
                    line: n,
                    fields: Vec::new(),
                    rows: 0,
                    text: Some(String::new()),
                });
            }
            marker = Some((kind, sv, msg));
            continue;
        }

        if starts_record(line) {
            let (kind, msg) = match &marker {
                Some((kind, _, msg)) if kind == "EPH" => (kind.clone(), msg.clone()),
                _ => (String::new(), String::new()),
            };
            let mut fields = Vec::new();
            for i in 0..3 {
                let start = 23 + FIELD_WIDTH * i;
                push_field(
                    &mut fields,
                    slice(line, start, start + FIELD_WIDTH),
                    n,
                    report,
                );
            }
            parsed.records.push(Record {
                kind,
                sv: slice(line, 0, 3).to_string(),
                msg,
                epoch: slice(line, 4, 23).trim().to_string(),
                line: n,
                fields,
                rows: 1,
                text: None,
            });
        } else if let Some(record) = parsed.records.last_mut() {
            if let Some(text) = record.text.as_mut() {
                if line.trim().is_empty() {
                    // free-form records ignore blank lines
                } else if record.epoch.is_empty() {
                    record.epoch = normalise(line);
                } else {
                    text.push_str(&normalise(line));
                    text.push(' ');
                }
            } else {
                for i in 0..FIELDS_PER_ROW {
                    let start = 4 + FIELD_WIDTH * i;
                    push_field(
                        &mut record.fields,
                        slice(line, start, start + FIELD_WIDTH),
                        n,
                        report,
                    );
                }
                record.rows += 1;
            }
        }
    }

    parsed
}

fn push_field(fields: &mut Vec<Option<f64>>, raw: &str, line: usize, report: &mut Report) {
    match parse_field(raw) {
        Ok(value) => fields.push(value),
        Err(()) => {
            report.error(
                format!("line {line}"),
                format!("field {raw:?} is not a number (columns misaligned?)"),
            );
            fields.push(None);
        }
    }
}

/// Layout checks that need no source file.
pub fn check_structure(output: &str) -> Report {
    let mut report = Report::new();
    let parsed = parse(output, &mut report);

    let mut rows_per_group: HashMap<String, usize> = HashMap::new();
    for record in parsed.records.iter().filter(|r| r.text.is_none()) {
        let group = format!("{}{}", record.sv.chars().next().unwrap_or('?'), record.msg);
        match rows_per_group.get(&group) {
            Some(&rows) if rows != record.rows => report.error(
                format!("line {}", record.line),
                format!(
                    "{} has {} line(s), other {group} records have {rows}",
                    record.id(),
                    record.rows
                ),
            ),
            Some(_) => {}
            None => {
                rows_per_group.insert(group, record.rows);
            }
        }
    }
    report
}

fn rinex_code(system: GnssSystem) -> char {
    match system {
        GnssSystem::Gps => 'G',
        GnssSystem::Glonass => 'R',
        GnssSystem::Galileo => 'E',
        GnssSystem::Beidou => 'C',
        GnssSystem::Qzss => 'J',
        GnssSystem::Sbas => 'S',
        GnssSystem::Irnss => 'I',
    }
}

fn close(x: f64, y: f64) -> bool {
    (x - y).abs() <= 1e-12 * x.abs().max(y.abs()).max(1e-30)
}

/// Slots the RINEX 3 layout leaves spare, where the source may still hold
/// a value: the NavIC orbit lines 5 and 6 end in spare slots (fields 21,
/// 23 and 27), and CDDIS broadcast files repeat IODEC in the last.
fn is_spare_slot(record: &Record, index: usize) -> bool {
    record.kind.is_empty() && record.sv.starts_with('I') && matches!(index, 20 | 22 | 26)
}

/// Positions where two ephemeris records differ. A blank never equals a
/// value, zero included: the layout defines which fields are spare (always
/// blank), so a blank where the source has a value is a lost value.
fn field_differences(a: &Record, b: &Record, written: &HashSet<(String, usize)>) -> Vec<String> {
    if a.text.is_some() || b.text.is_some() {
        let tokens = |r: &Record| -> Vec<String> {
            r.text
                .as_deref()
                .unwrap_or("")
                .split_whitespace()
                .map(str::to_string)
                .collect()
        };
        let (x, y) = (tokens(a), tokens(b));
        // An optional trailing field the source omits (the Klobuchar
        // region code) is written as a default: zero, or one for a QZSS
        // JAPN record, since RINEX 4.02 moved the region code into that
        // subtype. No data is added or changed.
        let japan = b.kind == "ION" && b.msg.ends_with(" JAPN");
        let is_default_extra = |i: usize| {
            i >= x.len()
                && match y[i].parse::<f64>() {
                    Ok(v) => v == 0.0 || (japan && v == 1.0),
                    Err(_) => false,
                }
        };
        return (0..x.len().max(y.len()))
            .filter(|&i| x.get(i) != y.get(i) && !is_default_extra(i))
            .map(|i| {
                format!(
                    "value {} is {:?}, source has {:?}",
                    i + 1,
                    y.get(i).map_or("", String::as_str),
                    x.get(i).map_or("", String::as_str)
                )
            })
            .collect();
    }
    let len = a.fields.len().max(b.fields.len());
    (0..len)
        .filter_map(|i| {
            let x = a.fields.get(i).copied().flatten();
            let y = b.fields.get(i).copied().flatten();
            match (x, y) {
                (None, None) => None,
                (Some(x), Some(y)) if close(x, y) => None,
                // a zero where the output never has a value, for any record
                // of that system and message type, is a spare slot
                (Some(x), None) if x == 0.0 && !written.contains(&(b.group(), i)) => None,
                (Some(_), None) if is_spare_slot(b, i) => None,
                (Some(x), None) => Some(format!("field {} is blank, source has {x:E}", i + 1)),
                (None, Some(y)) => Some(format!("field {} is {y:E}, source is blank", i + 1)),
                (Some(x), Some(y)) => Some(format!("field {} is {y:E}, source has {x:E}", i + 1)),
            }
        })
        .collect()
}

/// Checks that every record of `systems` in `source` appears in `output`
/// with the same values, and nothing else was added. Only meaningful when
/// both files have the same RINEX major version.
pub fn compare(source: &str, output: &str, systems: &[GnssSystem]) -> Report {
    let mut report = Report::new();
    let mut ignored = Report::new();
    let src = parse(source, &mut ignored);
    let out = parse(output, &mut ignored);

    if src.major != out.major {
        report.warning(
            "version",
            format!(
                "source is RINEX {} and output RINEX {}: records are not \
                 compared (source has {} records, output {})",
                src.major,
                out.major,
                src.records.len(),
                out.records.len()
            ),
        );
        return report;
    }

    let letters: Vec<char> = systems.iter().map(|s| rinex_code(*s)).collect();
    let wanted = |r: &&Record| r.sv.chars().next().is_some_and(|c| letters.contains(&c));

    if ALL_SYSTEMS.iter().all(|s| systems.contains(s)) {
        let mut lost: Vec<&str> = src
            .corr
            .iter()
            .filter(|kind| !out.corr.contains(kind))
            .map(String::as_str)
            .collect();
        lost.sort_unstable();
        lost.dedup();
        if !lost.is_empty() {
            report.error(
                "header",
                format!("header correction(s) lost: {}", lost.join(", ")),
            );
        } else if out.corr.len() < src.corr.len() {
            // several lines of one kind (a per-satellite BeiDou set, say):
            // the rinex crate keeps one
            report.warning(
                "header",
                format!(
                    "{} repeated header correction line(s) collapsed (source {}, output {})",
                    src.corr.len() - out.corr.len(),
                    src.corr.len(),
                    out.corr.len()
                ),
            );
        }
    }

    // Records are matched per key (satellite, epoch, message type). A
    // source can hold several records under one key (re-broadcasts of a
    // message); the rinex crate keeps one of them, so the output may hold
    // fewer than the source.
    let mut groups: HashMap<String, (Vec<&Record>, Vec<&Record>)> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    let mut written_positions: HashSet<(String, usize)> = HashSet::new();
    for record in out
        .records
        .iter()
        .filter(wanted)
        .filter(|r| r.text.is_none())
    {
        for (i, field) in record.fields.iter().enumerate() {
            if field.is_some() {
                written_positions.insert((record.group(), i));
            }
        }
    }

    for record in src.records.iter().filter(wanted) {
        let key = record.key();
        if !groups.contains_key(&key) {
            order.push(key.clone());
        }
        groups.entry(key).or_default().0.push(record);
    }
    for record in out.records.iter().filter(wanted) {
        let key = record.key();
        if !groups.contains_key(&key) {
            order.push(key.clone());
        }
        groups.entry(key).or_default().1.push(record);
    }

    for key in order {
        let (mut sources, outputs) = groups.remove(&key).unwrap_or_default();
        let had_output = !outputs.is_empty();

        for written in outputs {
            let exact = sources
                .iter()
                .position(|s| field_differences(s, written, &written_positions).is_empty());
            match (exact, sources.is_empty()) {
                (Some(pos), _) => {
                    sources.remove(pos);
                }
                (None, true) => report.error(
                    format!("line {}", written.line),
                    format!("{} appears in the output but not the source", written.id()),
                ),
                (None, false) => {
                    let closest = sources.remove(0);
                    let diffs = field_differences(closest, written, &written_positions);
                    let shown: Vec<&str> = diffs.iter().take(3).map(String::as_str).collect();
                    report.error(
                        format!("line {}", closest.line),
                        format!(
                            "{} differs in the output ({} field(s)): {}",
                            closest.id(),
                            diffs.len(),
                            shown.join("; ")
                        ),
                    );
                }
            }
        }

        for left_over in sources {
            if had_output {
                report.warning(
                    format!("line {}", left_over.line),
                    format!(
                        "{} is one of several records with the same satellite, epoch and \
                         message type; the output keeps another",
                        left_over.id()
                    ),
                );
            } else {
                report.error(
                    format!("line {}", left_over.line),
                    format!("{} is missing from the output", left_over.id()),
                );
            }
        }
    }
    report
}

/// Structure of `output` plus, when the major versions match, a record
/// comparison against `source`.
pub fn validate(source: &str, output: &str, systems: &[GnssSystem]) -> Report {
    let mut report = check_structure(output);
    report.extend(compare(source, output, systems));
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::systems::parse_systems;

    const HEADER: &str =
        "     3.04           NAVIGATION DATA     M                   RINEX VERSION / TYPE
                                                            END OF HEADER
";

    // A GLONASS-style record: 3 continuation lines, last one partly spare.
    const GOOD: &str =
        "R01 2025 04 13 00 15 00 1.213373616338E-04 9.094947017729E-13 0.000000000000E+00
    -1.297873828125E+04-2.191820144653E-01-1.862645149231E-09 0.000000000000E+00
    -2.192364160156E+04-3.801059722900E-02 1.862645149231E-09 1.000000000000E+00
    -1.023139648438E+03 3.573199272156E+00 0.000000000000E+00 0.000000000000E+00
";

    // The same record with the trailing zeros written as blanks (allowed:
    // trailing spare) ...
    const TRAILING_BLANK: &str =
        "R01 2025 04 13 00 15 00 1.213373616338E-04 9.094947017729E-13 0.000000000000E+00
    -1.297873828125E+04-2.191820144653E-01-1.862645149231E-09 0.000000000000E+00
    -2.192364160156E+04-3.801059722900E-02 1.862645149231E-09 1.000000000000E+00
    -1.023139648438E+03 3.573199272156E+00
";

    // ... and with a zero in the middle written as a blank (not allowed).
    const INTERIOR_BLANK: &str =
        "R01 2025 04 13 00 15 00 1.213373616338E-04 9.094947017729E-13 0.000000000000E+00
    -1.297873828125E+04-2.191820144653E-01-1.862645149231E-09 0.000000000000E+00
    -2.192364160156E+04                    1.862645149231E-09 1.000000000000E+00
    -1.023139648438E+03 3.573199272156E+00
";

    fn nav(body: &str) -> String {
        format!("{HEADER}{body}")
    }

    fn all() -> Vec<GnssSystem> {
        parse_systems("all").unwrap()
    }

    #[test]
    fn well_formed_record_passes() {
        assert!(!check_structure(&nav(GOOD)).has_errors());
    }

    #[test]
    fn trailing_blank_spare_fields_are_allowed() {
        assert!(!check_structure(&nav(TRAILING_BLANK)).has_errors());
    }

    #[test]
    fn column_shifted_line_is_an_error() {
        // The continuation line starts one column early, so a negative
        // value's sign lands in the next field.
        let shifted = GOOD.replace(
            "    -2.192364160156E+04-3.801059722900E-02",
            "   -2.192364160156E+04-3.801059722900E-02 ",
        );
        assert!(check_structure(&nav(&shifted)).has_errors());
    }

    #[test]
    fn overlong_line_is_an_error() {
        let long = GOOD.replace("1.000000000000E+00\n", "1.000000000000E+00 99\n");
        assert!(check_structure(&nav(&long)).has_errors());
    }

    #[test]
    fn identical_files_compare_clean() {
        let report = compare(&nav(GOOD), &nav(GOOD), &all());
        assert!(report.issues.is_empty(), "{:?}", report.render());
    }

    #[test]
    fn missing_record_is_reported() {
        let report = compare(&nav(GOOD), &nav(""), &all());
        assert_eq!(report.error_count(), 1);
        assert!(report.render()[0].contains("missing from the output"));
    }

    #[test]
    fn extra_record_is_reported() {
        let report = compare(&nav(""), &nav(GOOD), &all());
        assert_eq!(report.error_count(), 1);
        assert!(report.render()[0].contains("not the source"));
    }

    #[test]
    fn changed_value_is_reported() {
        let altered = GOOD.replace("3.573199272156E+00", "3.573299272156E+00");
        let report = compare(&nav(GOOD), &nav(&altered), &all());
        assert_eq!(report.error_count(), 1);
        assert!(report.render()[0].contains("differs in the output"));
    }

    #[test]
    fn flipped_sign_is_reported() {
        let flipped = GOOD.replace("-2.191820144653E-01", " 2.191820144653E-01");
        assert!(compare(&nav(GOOD), &nav(&flipped), &all()).has_errors());
    }

    #[test]
    fn zero_written_as_blank_is_reported_with_the_field() {
        let report = compare(&nav(GOOD), &nav(INTERIOR_BLANK), &all());
        assert_eq!(report.error_count(), 1, "{:?}", report.render());
        assert!(report.render()[0].contains("field 9 is blank, source has"));
    }

    #[test]
    fn trailing_zeros_blanked_where_other_records_have_values_are_reported() {
        // The same positions hold values in another record of the output,
        // so they are data fields, not spare slots.
        let other = GOOD.replace("R01", "R02");
        let source = nav(&format!("{GOOD}{other}"));
        let written = nav(&format!(
            "{TRAILING_BLANK}{}",
            other.replace("0.000000000000E+00\n", "1.000000000000E+00\n")
        ));
        let source = source.replace(
            &other,
            &other.replace("0.000000000000E+00\n", "1.000000000000E+00\n"),
        );
        let report = compare(&source, &written, &all());
        assert_eq!(report.error_count(), 1, "{:?}", report.render());
    }

    #[test]
    fn zero_in_a_slot_the_output_never_fills_is_a_spare() {
        // Every output record is blank there: the crate has no field for
        // the slot, as with the spare positions of GPS and IRNSS records.
        let report = compare(&nav(GOOD), &nav(TRAILING_BLANK), &all());
        assert!(!report.has_errors(), "{:?}", report.render());
    }

    #[test]
    fn spare_fields_blank_in_both_files_are_fine() {
        assert!(
            compare(&nav(TRAILING_BLANK), &nav(TRAILING_BLANK), &all())
                .issues
                .is_empty()
        );
    }

    #[test]
    fn records_of_unrequested_systems_are_not_expected() {
        let gps_only = parse_systems("gps").unwrap();
        let report = compare(&nav(GOOD), &nav(""), &gps_only);
        assert!(report.issues.is_empty());
    }

    #[test]
    fn lost_header_corrections_are_reported() {
        let with_corr =
            "     3.04           NAVIGATION DATA     M                   RINEX VERSION / TYPE
GPSA   3.0734e-08  1.4901e-08 -1.7881e-07 -5.9605e-08       IONOSPHERIC CORR
                                                            END OF HEADER
";
        let report = compare(with_corr, &nav(""), &all());
        assert_eq!(report.error_count(), 1);
        assert!(report.render()[0].contains("header correction(s) lost: GPSA"));
    }

    #[test]
    fn different_versions_are_not_compared_record_by_record() {
        let v4 = nav(GOOD).replacen("3.04", "4.00", 1);
        let report = compare(&nav(GOOD), &v4, &all());
        assert!(!report.has_errors());
        assert_eq!(report.warning_count(), 1);
    }

    const V4_HEADER: &str =
        "     4.00           NAVIGATION DATA     M                   RINEX VERSION / TYPE
                                                            END OF HEADER
";
    const ION_SOURCE: &str = "> ION G03 LNAV
    2026 10 01 23 56 24 1.490116119385e-08 7.450580596924e-09-1.192092895508e-07
    -5.960464477539e-08 1.064960000000e+05 3.276800000000e+04-2.621440000000e+05
    -6.553600000000e+04
";

    #[test]
    fn omitted_optional_trailing_zero_field_is_tolerated() {
        let written = ION_SOURCE.replace(
            "-6.553600000000e+04",
            "-6.553600000000e+04 0.000000000000e+00",
        );
        let report = compare(
            &format!("{V4_HEADER}{ION_SOURCE}"),
            &format!("{V4_HEADER}{written}"),
            &all(),
        );
        assert!(report.issues.is_empty(), "{:?}", report.render());
    }

    #[test]
    fn extra_nonzero_or_changed_free_form_values_are_still_errors() {
        let extra = ION_SOURCE.replace(
            "-6.553600000000e+04",
            "-6.553600000000e+04 1.000000000000e+00",
        );
        let changed = ION_SOURCE.replace("1.064960000000e+05", "1.064960000001e+05");
        for written in [extra, changed] {
            let report = compare(
                &format!("{V4_HEADER}{ION_SOURCE}"),
                &format!("{V4_HEADER}{written}"),
                &all(),
            );
            assert_eq!(report.error_count(), 1, "{:?}", report.render());
        }
    }

    const QZSS_ION: &str = "> ION J02 CNVX JAPN
    2026 08 01 00 03 12 1.303851604462e-08-2.235174179077e-08-1.192092895508e-07
     0.000000000000e+00 8.192000000000e+04 1.081344000000e+06-4.653056000000e+06
     7.405568000000e+06
";

    #[test]
    fn region_code_one_is_a_default_for_a_japan_ion_record() {
        let written = QZSS_ION.replace(
            "7.405568000000e+06",
            "7.405568000000e+06 1.000000000000e+00",
        );
        let report = compare(
            &format!("{V4_HEADER}{QZSS_ION}"),
            &format!("{V4_HEADER}{written}"),
            &all(),
        );
        assert!(report.issues.is_empty(), "{:?}", report.render());
    }

    #[test]
    fn region_code_one_is_still_an_error_outside_a_japan_ion_record() {
        let source = QZSS_ION.replace("JAPN", "WIDE");
        let written = source.replace(
            "7.405568000000e+06",
            "7.405568000000e+06 1.000000000000e+00",
        );
        let report = compare(
            &format!("{V4_HEADER}{source}"),
            &format!("{V4_HEADER}{written}"),
            &all(),
        );
        assert_eq!(report.error_count(), 1, "{:?}", report.render());
    }

    // A NavIC RINEX 3 record: IODEC repeated in the last spare slot of
    // line 6 in the source, written blank in the output.
    const NAVIC: &str =
        "I02 2026 08 01 06 20 48 4.145773127675E-04 3.910827217624E-11 0.000000000000E+00
     1.860000000000E+02 1.587500000000E+01 4.968778398136E-09 5.264849237646E-01
     4.656612873077E-07 2.147695166059E-03-1.459568738937E-05 6.493429954529E+03
     5.412480000000E+05 1.043081283569E-07-1.116442215934E+00-6.332993507385E-08
     5.027698089406E-01 5.303125000000E+02-2.964699295603E+00-4.515902391175E-09
    -6.357407668690E-11 0.000000000000E+00 2.429000000000E+03 0.000000000000E+00
     2.000000000000E+00 0.000000000000E+00-1.862645000000E-09 1.860000000000E+02
     5.414040000000E+05
";

    #[test]
    fn navic_spare_slot_holding_a_value_may_be_blank_in_the_output() {
        let written = NAVIC.replace(
            "-1.862645000000E-09 1.860000000000E+02",
            "-1.862645000000E-09                   ",
        );
        let report = compare(&nav(NAVIC), &nav(&written), &all());
        assert!(!report.has_errors(), "{:?}", report.render());
    }

    #[test]
    fn navic_data_field_blanked_in_the_output_is_still_an_error() {
        // the SV health slot of line 6 is not spare
        let written = NAVIC.replace(
            "2.000000000000E+00 0.000000000000E+00-1.862645000000E-09",
            "2.000000000000E+00 1.000000000000E+00-1.862645000000E-09",
        );
        let blanked = written.replace(
            "2.000000000000E+00 1.000000000000E+00",
            "2.000000000000E+00                   ",
        );
        let source = nav(&written);
        let report = compare(&source, &nav(&blanked), &all());
        assert_eq!(report.error_count(), 1, "{:?}", report.render());
    }

    #[test]
    fn collapsed_duplicate_records_are_a_warning_when_the_survivor_matches() {
        let other = GOOD.replace("3.573199272156E+00", "3.573299272156E+00");
        let source = nav(&format!("{GOOD}{other}"));
        // the output keeps the second of the two records
        let report = compare(&source, &nav(&other), &all());
        assert!(!report.has_errors(), "{:?}", report.render());
        assert_eq!(report.warning_count(), 1);
    }

    #[test]
    fn a_surviving_duplicate_that_matches_no_source_record_is_an_error() {
        let a = GOOD.replace("3.573199272156E+00", "3.573299272156E+00");
        let b = GOOD.replace("3.573199272156E+00", "3.573399272156E+00");
        let altered = GOOD.replace("3.573199272156E+00", "3.573499272156E+00");
        let report = compare(&nav(&format!("{a}{b}")), &nav(&altered), &all());
        assert_eq!(report.error_count(), 1, "{:?}", report.render());
    }

    #[test]
    fn collapsed_repeats_of_one_header_correction_are_a_warning() {
        let line = "BDSA   2.7008E-08  0.0000E+00 -7.7486E-07  1.4305E-06       IONOSPHERIC CORR\n";
        let other =
            "BDSA   2.4214E-08  0.0000E+00 -7.7486E-07  1.4305E-06       IONOSPHERIC CORR\n";
        let with = |lines: &str| {
            format!(
                "     3.04           NAVIGATION DATA     M                   RINEX VERSION / TYPE\n\
                 {lines}                                                            END OF HEADER\n"
            )
        };
        let report = compare(&with(&format!("{line}{other}")), &with(line), &all());
        assert!(!report.has_errors(), "{:?}", report.render());
        assert_eq!(report.warning_count(), 1);
    }
}
