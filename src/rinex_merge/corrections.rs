//! Header corrections of a navigation file and RINEX 4 records, both ways.
//!
//! RINEX 4 has no `TIME SYSTEM CORR` and `IONOSPHERIC CORR` header lines:
//! the system time offsets are STO records and the ionosphere models ION
//! records ("The STO messages replace the previous SYSTEM TIME CORR header
//! line(s)", RINEX 4.02 section 5.4.9). The rinex crate writes a header
//! without them when the version changes from 3 to 4, so the corrections
//! are turned into records here, before the file is written. Going from
//! RINEX 4 to 3, the records are turned back into header lines.

use rinex::navigation::{
    IonosphereModel, KbModel, KbRegionCode, NavFrame, NavFrameType, NavKey, NavMessageType,
    NgModel, NgRegionFlags, StoIdentity, TimeOffset, TransmissionTime,
};
use rinex::prelude::{Constellation, Epoch, Rinex, SV, TimeScale};

use crate::systems::{GnssSystem, matches_constellation};

const NANOS_PER_SECOND: u64 = 1_000_000_000;

/// The satellite system that broadcasts a time offset, from the first two
/// letters of its four letter code (`GLGP` is broadcast by GLONASS).
fn broadcaster(code: &str) -> Option<(Constellation, NavMessageType)> {
    match code.get(..2)? {
        "GP" => Some((Constellation::GPS, NavMessageType::LNAV)),
        "GL" => Some((Constellation::Glonass, NavMessageType::FDMA)),
        "GA" => Some((Constellation::Galileo, NavMessageType::IFNV)),
        "BD" => Some((Constellation::BeiDou, NavMessageType::D1D2)),
        "QZ" => Some((Constellation::QZSS, NavMessageType::LNAV)),
        "IR" => Some((Constellation::IRNSS, NavMessageType::LNAV)),
        _ => None,
    }
}

fn requested(systems: &[GnssSystem], constellation: Constellation) -> bool {
    systems
        .iter()
        .any(|system| matches_constellation(*system, constellation))
}

/// Reference epoch of a header time offset: `seconds` into week `week`.
/// The week is a continuous GPS week (GPS, GAL, QZS, IRN) or, for BeiDou,
/// a week continuous from 1 Jan 2006. Some producers write a GPS week on
/// the BeiDou line, so for BeiDou the reading closer to `near`, an epoch of
/// the file, is taken. GLONASS lines may carry zero for both (RINEX 3.04
/// table A5); they have no reference time, and `near` stands in for it.
fn reference_epoch(code: &str, week: u32, seconds: u64, near: Epoch) -> Epoch {
    if week == 0 && seconds == 0 {
        return near;
    }
    let nanos = seconds * NANOS_PER_SECOND;
    let gps = Epoch::from_time_of_week(week, nanos, TimeScale::GPST);
    if !code.starts_with("BD") {
        return gps;
    }
    let bds = Epoch::from_time_of_week(week, nanos, TimeScale::BDT);
    let distance = |epoch: Epoch| (epoch - near).abs();
    if distance(bds) < distance(gps) {
        bds
    } else {
        gps
    }
}

/// A STO frame for a header `TIME SYSTEM CORR` line, or `None` when the
/// line has no RINEX 4 form (an SBAS offset needs an SBAS identifier that
/// the header line does not give).
fn sto_frame(offset: &TimeOffset, near: Epoch) -> Option<(NavKey, NavFrame)> {
    let code = offset.time_system.as_deref()?;
    if code.len() != 4 || !code.is_ascii() {
        return None;
    }
    let (constellation, msgtype) = broadcaster(code)?;
    let seconds = offset.t_ref.1 / NANOS_PER_SECOND;
    let reference = reference_epoch(code, offset.t_ref.0, seconds, near);

    let mut sto = TimeOffset::from_epoch(reference, offset.lhs, offset.rhs, offset.polynomial);
    sto.time_system = offset.time_system.clone();
    // the header line has no transmission time; RINEX 4 wants one
    sto.transmission_time = Some(seconds as f64);

    let mut time_system = [b' '; 4];
    time_system.copy_from_slice(code.as_bytes());
    let key = NavKey {
        epoch: Epoch::from_time_of_week(sto.t_ref.0, sto.t_ref.1, sto.lhs),
        // no satellite number: the correction is not tied to one satellite
        sv: SV::new(constellation, 0),
        msgtype,
        frmtype: NavFrameType::SystemTimeOffset,
        subtype: None,
        galileo_data_sources: None,
        transmission_time: Some(TransmissionTime::new(seconds as f64).ok()?),
        sto_identity: Some(StoIdentity {
            time_system,
            sbas_id: [b' '; 18],
            utc_id: [b' '; 18],
        }),
    };
    Some((key, NavFrame::STO(sto)))
}

/// One `IONOSPHERIC CORR` header line (RINEX 3.04 table A5): the four
/// values, and for BeiDou a time mark and a satellite number that tell
/// apart the many models a header can hold.
struct IonLine {
    code: String,
    values: Vec<f64>,
    /// Hour of the BDT day of the transmission, `A` (00h) to `X` (23h).
    mark: Option<char>,
    sv: Option<u8>,
}

fn ion_lines(source: &str) -> Vec<IonLine> {
    source
        .lines()
        .take_while(|line| !line.contains("END OF HEADER"))
        .filter(|line| {
            line.get(60..)
                .is_some_and(|l| l.trim() == "IONOSPHERIC CORR")
        })
        .filter_map(|line| {
            let values = (0..4)
                .filter_map(|i| {
                    let field = line.get(5 + 12 * i..17 + 12 * i)?.trim();
                    field.replace(['D', 'd'], "E").parse::<f64>().ok()
                })
                .collect();
            Some(IonLine {
                code: line.get(..4)?.trim().to_string(),
                values,
                mark: line
                    .get(54..55)
                    .and_then(|m| m.trim().chars().next())
                    .filter(|m| ('A'..='X').contains(m)),
                sv: line.get(56..58).and_then(|n| n.trim().parse().ok()),
            })
        })
        .collect()
}

/// (system code, time mark, satellite) of a header model, and its values.
type IonGroups =
    std::collections::BTreeMap<(String, Option<char>, Option<u8>), (Vec<f64>, Vec<f64>)>;

type IonEntry = (Constellation, Option<char>, Option<u8>, IonosphereModel);

/// The ionosphere models of the header lines of `source`, with the system
/// they are for and the key (time mark, satellite) that tells apart the
/// several models of one system.
fn ion_models(source: &str) -> Vec<IonEntry> {
    // alpha (or Galileo ai) and beta values of each group
    let mut groups = IonGroups::new();
    for line in ion_lines(source) {
        let system = line.code.trim_end_matches(['A', 'B']).to_string();
        let entry = groups.entry((system, line.mark, line.sv)).or_default();
        if line.code.ends_with('B') {
            entry.1 = line.values;
        } else {
            entry.0 = line.values;
        }
    }

    let value = |values: &[f64], i: usize| values.get(i).copied().unwrap_or(0.0);
    let four = |v: &[f64]| (value(v, 0), value(v, 1), value(v, 2), value(v, 3));
    groups
        .into_iter()
        .filter_map(|((system, mark, sv), (a, b))| {
            let (constellation, model) = match system.as_str() {
                "GAL" => (
                    Constellation::Galileo,
                    IonosphereModel::NequickG(NgModel {
                        a: (value(&a, 0), value(&a, 1), value(&a, 2)),
                        region: NgRegionFlags::default(),
                    }),
                ),
                "GPS" | "BDS" | "QZS" | "IRN" => {
                    let constellation = match system.as_str() {
                        "GPS" => Constellation::GPS,
                        "BDS" => Constellation::BeiDou,
                        "QZS" => Constellation::QZSS,
                        _ => Constellation::IRNSS,
                    };
                    let region = if constellation == Constellation::QZSS {
                        KbRegionCode::Japan
                    } else {
                        KbRegionCode::Worldwide
                    };
                    (
                        constellation,
                        IonosphereModel::Klobuchar(KbModel {
                            alpha: four(&a),
                            beta: four(&b),
                            region,
                        }),
                    )
                }
                _ => return None,
            };
            Some((constellation, mark, sv, model))
        })
        .collect()
}

/// The message type an ionosphere model of `constellation` is filed under
/// in RINEX 4.
fn ion_message_type(constellation: Constellation) -> Option<NavMessageType> {
    match constellation {
        Constellation::GPS | Constellation::QZSS | Constellation::IRNSS => {
            Some(NavMessageType::LNAV)
        }
        Constellation::BeiDou => Some(NavMessageType::D1D2),
        Constellation::Galileo => Some(NavMessageType::IFNV),
        _ => None,
    }
}

/// The calendar day most ephemerides of the file are dated in, in `scale`:
/// the day the file is for, though it starts with a few ephemerides of the
/// day before.
fn file_day(nav: &rinex::navigation::Record, scale: TimeScale) -> Option<(i32, u8, u8)> {
    let mut days: std::collections::HashMap<(i32, u8, u8), usize> = Default::default();
    for key in nav.keys().filter(|k| k.frmtype == NavFrameType::Ephemeris) {
        let (year, month, day, ..) = key.epoch.to_gregorian(scale);
        *days.entry((year, month, day)).or_default() += 1;
    }
    days.into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|(day, _)| day)
}

/// Adds a STO frame for every `TIME SYSTEM CORR` line and an ION frame for
/// every `IONOSPHERIC CORR` model of the header of the RINEX 2/3 file
/// `source`, for the requested systems, so that they are written as
/// records at RINEX 4. The header lines are read from `source` because the
/// rinex crate keeps one model per system, while BeiDou lines come one per
/// satellite and hour. A BeiDou model is dated at the hour of its time
/// mark and has its satellite; the others are dated at the first ephemeris
/// of the file, as their header lines have no epoch. Returns the number of
/// frames added.
pub(crate) fn add_header_corrections(
    rinex: &mut Rinex,
    systems: &[GnssSystem],
    source: &str,
) -> usize {
    let Some(nav) = rinex.record.as_nav() else {
        return 0;
    };
    let Some(first) = nav
        .keys()
        .filter(|key| key.frmtype == NavFrameType::Ephemeris)
        .map(|key| key.epoch)
        .min()
    else {
        return 0;
    };
    let bds_day = file_day(nav, TimeScale::BDT);

    let offsets = rinex
        .header
        .nav
        .as_ref()
        .map(|nav| nav.time_offsets.clone())
        .unwrap_or_default();

    let Some(nav) = rinex.record.as_mut_nav() else {
        return 0;
    };
    let mut added = 0;

    for offset in &offsets {
        let wanted = offset
            .time_system
            .as_deref()
            .and_then(broadcaster)
            .is_some_and(|(constellation, _)| requested(systems, constellation));
        if !wanted {
            continue;
        }
        if let Some((key, frame)) = sto_frame(offset, first) {
            nav.insert(key, frame);
            added += 1;
        }
    }

    for (constellation, mark, sv, model) in ion_models(source) {
        if !requested(systems, constellation) {
            continue;
        }
        let Some(msgtype) = ion_message_type(constellation) else {
            continue;
        };
        let scale = constellation.timescale().unwrap_or(first.time_scale);
        let epoch = match (mark, bds_day) {
            (Some(mark), Some((year, month, day))) if constellation == Constellation::BeiDou => {
                let hour = mark as u8 - b'A';
                Epoch::from_gregorian(year, month, day, hour, 0, 0, 0, TimeScale::BDT)
            }
            _ => first.to_time_scale(scale),
        };
        let key = NavKey {
            epoch,
            sv: SV::new(constellation, sv.unwrap_or(0)),
            msgtype,
            frmtype: NavFrameType::IonosphereModel,
            subtype: None,
            galileo_data_sources: None,
            transmission_time: None,
            sto_identity: None,
        };
        nav.insert(key, NavFrame::ION(model));
        added += 1;
    }
    added
}

/// Seconds into the week and week of `epoch`, as a header correction line
/// holds them: a continuous GPS week, or a BeiDou week for BeiDou.
fn header_week(code: &str, epoch: Epoch) -> (u32, u64) {
    let scale = if code.starts_with("BD") {
        TimeScale::BDT
    } else {
        TimeScale::GPST
    };
    epoch.to_time_scale(scale).to_time_of_week()
}

/// Adds a `TIME SYSTEM CORR` header line for every STO frame code and an
/// `IONOSPHERIC CORR` model for every constellation of a RINEX 4 file, so
/// that they are written to the header at RINEX 3. A header holds one line
/// per code, so the first frame of each code (the earliest) is taken. Not
/// converted: offsets with an SBAS identifier, and ionosphere models other
/// than Klobuchar and NeQuick-G, which the RINEX 3 header cannot hold.
/// Returns the number of header entries added.
pub(crate) fn add_header_corrections_from_records(rinex: &mut Rinex) -> usize {
    use rinex::navigation::IonosphereModel;

    let mut offsets: Vec<TimeOffset> = Vec::new();
    let mut ions: Vec<(Constellation, IonosphereModel)> = Vec::new();
    if let Some(nav) = rinex.record.as_nav() {
        for (key, frame) in nav {
            match frame {
                NavFrame::STO(sto) => {
                    let Some(code) = sto.time_system.as_deref() else {
                        continue;
                    };
                    let has_sbas = sto.sbas.as_deref().is_some_and(|id| !id.trim().is_empty());
                    if broadcaster(code).is_none()
                        || has_sbas
                        || offsets
                            .iter()
                            .any(|o| o.time_system.as_deref() == Some(code))
                    {
                        continue;
                    }
                    let (week, nanos) = header_week(code, key.epoch);
                    let mut offset = TimeOffset::from_time_of_week(
                        week,
                        nanos,
                        sto.lhs,
                        sto.rhs,
                        (sto.polynomial.0, sto.polynomial.1, 0.0),
                    );
                    offset.time_system = Some(code.to_string());
                    offsets.push(offset);
                }
                NavFrame::ION(model)
                    if matches!(
                        model,
                        IonosphereModel::Klobuchar(_) | IonosphereModel::NequickG(_)
                    ) && !ions.iter().any(|(c, _)| *c == key.sv.constellation) =>
                {
                    ions.push((key.sv.constellation, *model));
                }
                _ => {}
            }
        }
    }

    let added = offsets.len() + ions.len();
    let header = rinex.header.nav.get_or_insert_with(Default::default);
    for offset in offsets {
        header.add_time_offset(offset);
    }
    for (constellation, model) in ions {
        rinex.header.ionod_corrections.insert(constellation, model);
    }
    added
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near() -> Epoch {
        Epoch::from_gregorian_utc_at_midnight(2025, 4, 13).to_time_scale(TimeScale::GPST)
    }

    #[test]
    fn gps_week_is_read_from_the_gps_origin() {
        let epoch = reference_epoch("GPUT", 2362, 319_488, near());
        assert_eq!(epoch.to_gregorian_utc().0, 2025);
    }

    #[test]
    fn beidou_week_from_either_origin_lands_next_to_the_file() {
        // 2362 is the GPS week of 2025-04-13, 1006 the BeiDou week; both
        // forms occur in files
        let gps_week = reference_epoch("BDUT", 2362, 14, near());
        let bds_week = reference_epoch("BDUT", 1006, 14, near());
        for epoch in [gps_week, bds_week] {
            assert_eq!(epoch.to_gregorian_utc().0, 2025, "{epoch}");
        }
    }

    #[test]
    fn zero_glonass_reference_falls_back_to_the_file_epoch() {
        assert_eq!(reference_epoch("GLUT", 0, 0, near()), near());
    }
}
