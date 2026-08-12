//! Resolves remote CDDIS paths for the combined broadcast nav product and
//! per-station obs products, including the `--time latest` product-tier
//! fallback (final, then rapid) for nav.
//!
//! Obs has no tier concept: CDDIS publishes one daily file per station,
//! straight from that station's own receiver submission (`R` source flag,
//! confirmed against the live archive), not a derived/combined product
//! with a final-vs-rapid distinction.

use crate::time::GpsDay;

/// How many days `--time latest` walks backward looking for a published
/// product, past the anchor (today) day. Generously covers the case where
/// even the rapid tier hasn't published yet for the most recent complete
/// day (see plan §8: rapid is typically available ~3h after day close).
const MAX_LATEST_DAYS_BACK: u32 = 5;

/// Combined broadcast nav product tiers. There are two real tiers, not the
/// three ("final/rapid/ultra-rapid") the original plan assumed — that
/// terminology is for IGS orbit/clock products and doesn't apply to
/// broadcast nav. Confirmed against the live archive (plan §8): `Final`
/// publishes ~9h after day close, `Rapid` ~3h after day close.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavTier {
    /// `BRDC00IGS`, the IGS-combined product.
    Final,
    /// `BRD400DLR`, the DLR real-time-stream combined product. Already
    /// RINEX 4, so no upconversion needed when this tier is used.
    Rapid,
}

impl NavTier {
    fn product_id(self) -> &'static str {
        match self {
            NavTier::Final => "BRDC00IGS_R",
            NavTier::Rapid => "BRD400DLR_S",
        }
    }
}

/// A single combined-nav download to attempt: which day, which tier, and
/// the resulting CDDIS URL.
#[derive(Debug, Clone, PartialEq)]
pub struct NavCandidate {
    pub day: GpsDay,
    pub tier: NavTier,
    pub url: String,
}

fn nav_url(day: GpsDay, tier: NavTier) -> String {
    format!(
        "https://cddis.nasa.gov/archive/gnss/data/daily/{year:04}/{doy:03}/{yy:02}p/{product}_{year:04}{doy:03}0000_01D_MN.rnx.gz",
        year = day.year,
        doy = day.day_of_year,
        yy = day.year.rem_euclid(100),
        product = tier.product_id(),
    )
}

/// Candidates for an explicit `--time <ISO8601>` day: final, then rapid,
/// for that exact day only. No day-walking, since a specific day was
/// requested.
pub fn nav_candidates_for_day(day: GpsDay) -> Vec<NavCandidate> {
    [NavTier::Final, NavTier::Rapid]
        .into_iter()
        .map(|tier| NavCandidate {
            day,
            tier,
            url: nav_url(day, tier),
        })
        .collect()
}

/// Candidates for `--time latest`: final then rapid at `anchor`, then the
/// same pair at each of the `MAX_LATEST_DAYS_BACK` preceding days, in
/// order. The caller tries these in sequence and stops at the first one
/// that actually exists on CDDIS.
pub fn nav_candidates_for_latest(anchor: GpsDay) -> Vec<NavCandidate> {
    (0..=MAX_LATEST_DAYS_BACK)
        .flat_map(|days_back| nav_candidates_for_day(anchor.days_before(days_back)))
        .collect()
}

/// CDDIS URL for `station`'s (already-validated, 9-character) daily obs
/// product on `day`: 30s sampling, mixed (all-constellation) observation
/// data, Hatanaka-compressed (`Rinex::parse` decompresses this
/// transparently — no separate step needed).
pub fn obs_url(day: GpsDay, station: &str) -> String {
    format!(
        "https://cddis.nasa.gov/archive/gnss/data/daily/{year:04}/{doy:03}/{yy:02}d/{station}_R_{year:04}{doy:03}0000_01D_30S_MO.crx.gz",
        year = day.year,
        doy = day.day_of_year,
        yy = day.year.rem_euclid(100),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn day_candidates_are_final_then_rapid() {
        let day = GpsDay::resolve("2026-08-01").unwrap();
        let candidates = nav_candidates_for_day(day);

        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].tier, NavTier::Final);
        assert_eq!(
            candidates[0].url,
            "https://cddis.nasa.gov/archive/gnss/data/daily/2026/213/26p/BRDC00IGS_R_20262130000_01D_MN.rnx.gz"
        );
        assert_eq!(candidates[1].tier, NavTier::Rapid);
        assert_eq!(
            candidates[1].url,
            "https://cddis.nasa.gov/archive/gnss/data/daily/2026/213/26p/BRD400DLR_S_20262130000_01D_MN.rnx.gz"
        );
    }

    #[test]
    fn latest_candidates_walk_backward_by_day_then_tier() {
        let anchor = GpsDay::resolve("2026-08-12").unwrap();
        let candidates = nav_candidates_for_latest(anchor);

        assert_eq!(candidates.len() as u32, 2 * (MAX_LATEST_DAYS_BACK + 1));
        assert_eq!(candidates[0].day, anchor);
        assert_eq!(candidates[0].tier, NavTier::Final);
        assert_eq!(candidates[1].day, anchor);
        assert_eq!(candidates[1].tier, NavTier::Rapid);
        assert_eq!(candidates[2].day, anchor.days_before(1));
        assert_eq!(candidates[2].tier, NavTier::Final);
    }

    #[test]
    fn obs_url_matches_expected_convention() {
        let day = GpsDay::resolve("2026-08-01").unwrap();
        assert_eq!(
            obs_url(day, "WTZR00DEU"),
            "https://cddis.nasa.gov/archive/gnss/data/daily/2026/213/26d/WTZR00DEU_R_20262130000_01D_30S_MO.crx.gz"
        );
    }
}
