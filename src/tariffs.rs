// Unlike most tariffs, Agile Octopus returns pricing data for every 30-minute slot.
// All the others give a flat rate for the duration of the product (usually 3 months,
// unless it is a 12 or 18 month fixed rate).
//
// Smart tariffs, aka Time-of-use tariffs, have multiple rates. These are usually
// published as a block. So we have to unpack them so that any tariff can be represented
// like Agile's 30-minute slots.
//
// Intelligent tariffs might _change_ the published cost of the slots based on usage.
// For example, on IOG, a smart-charge outside of the overnight period will also be charged
// at the cheap rate.
//
// Every tariff is stored per slot so that switching tariff never changes the shape of the pricing queries.
use chrono::{
    DateTime,
    Days,
    Duration,
    DurationRound,
    NaiveTime,
    TimeZone,
    Utc,
};
use chrono_tz::{
    Europe::London,
    Tz,
};
use eyre::{Error, eyre};
use crate::{
    dates::str2dt,
    times::TimeRange,
};

const SLOT: Duration = Duration::minutes(30);

#[derive(Clone, Debug, PartialEq)]
pub struct Rate {
    pub value: f32,
    pub valid_from: Option<DateTime<Utc>>,
    pub valid_to: Option<DateTime<Utc>>,
}

impl Rate {
    pub fn from_api(value: f32, valid_from: Option<&str>, valid_to: Option<&str>) -> Result<Self, Error> {
        Ok(Self {
            value,
            valid_from: valid_from.map(str2dt).transpose()?,
            valid_to: valid_to.map(str2dt).transpose()?,
        })
    }

    fn covers(&self, t: DateTime<Utc>) -> bool {
        self.valid_from.is_none_or(|f| f <= t) && self.valid_to.is_none_or(|e| t < e)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Slot {
    pub valid_from: DateTime<Utc>,
    pub valid_to: DateTime<Utc>,
    pub value: f32,
    pub standing_charge: f32,
}

/// The latest-starting rate covering `t`
pub fn rate_at(rates: &[Rate], t: DateTime<Utc>) -> Option<f32> {
    rates.iter()
        .filter(|r| r.covers(t))
        .max_by_key(|r| r.valid_from)
        .map(|r| r.value)
}

/// Slot starts within `window`, aligned to the half-hour.
pub fn half_hours(window: &TimeRange) -> impl Iterator<Item = DateTime<Utc>> {
    let first = window.start.duration_trunc(SLOT).expect("30m truncation");
    let first = if first < window.start { first + SLOT } else { first };
    let end = window.end;
    std::iter::successors(Some(first), |t| Some(*t + SLOT))
        .take_while(move |t| *t < end)
}

/// Open-ended agreements are expanded up to the end of tomorrow.
/// Day-ahead prices (like Agile) are published in the afternoon for the next day.
pub fn end_of_tomorrow(now: DateTime<Utc>) -> DateTime<Utc> {
    let day_after = now.with_timezone(&London).date_naive() + Days::new(2);
    let midnight = day_after.and_time(NaiveTime::MIN);
    // UK clocks change at 01:00/02:00, so local midnight always exists once.
    London.from_local_datetime(&midnight)
        .earliest()
        .expect("UK midnight is never skipped")
        .with_timezone(&Utc)
}

pub fn expand_half_hours(rates: &[Rate], standing_charges: &[Rate], window: &TimeRange) -> Vec<Slot> {
    half_hours(window)
        .filter_map(|t| Some(Slot {
            valid_from: t,
            valid_to: t + SLOT,
            value: rate_at(rates, t)?,
            standing_charge: rate_at(standing_charges, t).unwrap_or_default(),
        }))
        .collect()
}

/// Which of a two-rate tariff's unit-rate endpoints a timeslot bills at.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TimeslotRate {
    Day,
    Night,
    // Cosy,
    // Flux,
}

/// A time-of-use window from the agreement's Kraken `timeOfUseScheme`, in
/// local wall-clock time, so it follows clock changes.
#[derive(Clone, Debug, PartialEq)]
pub struct Timeslot {
    pub rate: TimeslotRate,
    pub timezone: Tz,
    pub active_from: NaiveTime,
    pub active_to: NaiveTime,
}

impl Timeslot {
    /// `name` is Kraken's timeslot name (e.g. `ECO7_NIGHT`); it is the only
    /// link between a window and the REST day/night unit-rate endpoints.
    pub fn new(name: &str, timezone: &str, active_from: &str, active_to: &str) -> Result<Self, Error> {
        let rate = if name.ends_with("NIGHT") {
            TimeslotRate::Night
        } else if name.ends_with("DAY") {
            TimeslotRate::Day
        } else {
            return Err(eyre!("timeslot {name} is neither a day nor a night rate"));
        };
        Ok(Self {
            rate,
            timezone: timezone.parse().map_err(|e| eyre!("timezone {timezone}: {e}"))?,
            active_from: NaiveTime::parse_from_str(active_from, "%H:%M:%S")?,
            active_to: NaiveTime::parse_from_str(active_to, "%H:%M:%S")?,
        })
    }

    /// `active_to` before `active_from` spans midnight.
    pub fn contains(&self, t: DateTime<Utc>) -> bool {
        let local = t.with_timezone(&self.timezone).time();
        if self.active_from <= self.active_to {
            self.active_from <= local && local < self.active_to
        } else {
            local >= self.active_from || local < self.active_to
        }
    }
}

/// The rate of the timeslot covering `t`.
pub fn timeslot_rate(timeslots: &[Timeslot], t: DateTime<Utc>) -> Option<TimeslotRate> {
    timeslots.iter().find(|s| s.contains(t)).map(|s| s.rate)
}

/// Whether a dispatch is smart charging.
/// Bump charges are user-requested and stay at the day rate.
pub fn is_discounted_dispatch(source: Option<&str>) -> bool {
    match source {
        None | Some("smart-charge") | Some("unknown") => true,
        Some(_) => false
    }
}

/// Two-rate tariffs (Intelligent Octopus Go), whose day/night windows come
/// from `timeslots`. Any half-hour touched by a smart-charge dispatch is
/// billed at the night rate for the whole home, and for the whole slot even if
/// the dispatch is shorter.
///
/// The T&Cs (§2.3.1.4) say only the car's kWh is discounted outside the
/// window, but Octopus's tariff notes say the whole home is; this follows the
/// notes. The 6-hours-per-24 cap on smart charging (§2.3.1.8) isn't applied.
/// https://octopus.energy/policies/smart-tariffs-terms-and-condition/
pub fn expand_day_night(
    day: &[Rate],
    night: &[Rate],
    timeslots: &[Timeslot],
    standing_charges: &[Rate],
    window: &TimeRange,
    dispatches: &[TimeRange],
) -> Result<Vec<Slot>, Error> {
    let mut slots = Vec::new();
    for t in half_hours(window) {
        let end = t + SLOT;
        let dispatched = dispatches.iter().any(|d| d.start < end && d.end > t);
        let rate = timeslot_rate(timeslots, t)
            .ok_or_else(|| eyre!("no timeslot covers {t}"))?;
        let rates = match rate {
            _ if dispatched => night,
            TimeslotRate::Night => night,
            TimeslotRate::Day => day,
        };
        if let Some(value) = rate_at(rates, t) {
            slots.push(Slot {
                valid_from: t,
                valid_to: end,
                value,
                standing_charge: rate_at(standing_charges, t).unwrap_or_default(),
            });
        }
    }
    Ok(slots)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dt(s: &str) -> DateTime<Utc> {
        str2dt(s).unwrap()
    }

    fn range(start: &str, end: &str) -> TimeRange {
        TimeRange::new_from_strs(start, end).unwrap()
    }

    fn rate(value: f32, from: Option<&str>, to: Option<&str>) -> Rate {
        Rate::from_api(value, from, to).unwrap()
    }

    #[test]
    fn horizon_is_local_midnight_after_tomorrow() {
        assert_eq!(end_of_tomorrow(dt("2026-11-22T09:00Z")), dt("2026-11-24T00:00Z"));
        assert_eq!(end_of_tomorrow(dt("2027-06-01T09:00Z")), dt("2027-06-02T23:00Z"), "BST midnight");
        // 23:30Z on 2027-06-01 is already 2nd June locally.
        assert_eq!(end_of_tomorrow(dt("2027-06-01T23:30Z")), dt("2027-06-03T23:00Z"));
    }

    #[test]
    fn flexible_single_rate_fills_every_slot_clipped_to_the_window() {
        let rates = [rate(26.5, Some("2025-11-01T00:00Z"), None)];
        let standing = [rate(47.9, Some("2025-11-01T00:00Z"), None)];
        // Agreement edges, e.g. VAR-22-11-01 from 5th to 27th December 2025.
        let slots = expand_half_hours(&rates, &standing, &range("2025-12-05T00:00Z", "2025-12-27T00:00Z"));

        assert_eq!(slots.len(), 22 * 48);
        assert_eq!(slots[0].valid_from, dt("2025-12-05T00:00Z"));
        assert_eq!(slots.last().unwrap().valid_to, dt("2025-12-27T00:00Z"));
        assert!(slots.iter().all(|s| s.value == 26.5 && s.standing_charge == 47.9));
    }

    #[test]
    fn later_rate_wins_where_ranges_overlap() {
        // A price change published as a new open-ended row.
        let rates = [
            rate(26.5, Some("2025-11-01T00:00Z"), None),
            rate(27.9, Some("2026-01-01T00:00Z"), None),
        ];
        let slots = expand_half_hours(&rates, &[], &range("2025-12-31T23:30Z", "2026-01-01T00:30Z"));
        assert_eq!(slots.iter().map(|s| s.value).collect::<Vec<_>>(), [26.5, 27.9]);
    }

    #[test]
    fn slots_without_a_rate_are_skipped() {
        let rates = [rate(10.0, Some("2026-11-22T00:00Z"), Some("2026-11-22T01:00Z"))];
        let slots = expand_half_hours(&rates, &[], &range("2026-11-21T23:00Z", "2026-11-22T02:00Z"));
        assert_eq!(slots.len(), 2);
    }

    #[test]
    fn unaligned_window_starts_at_the_next_half_hour() {
        let slots: Vec<_> = half_hours(&range("2026-11-22T00:10Z", "2026-11-22T01:30Z")).collect();
        assert_eq!(slots, [dt("2026-11-22T00:30Z"), dt("2026-11-22T01:00Z")]);
    }

    fn iog_rates() -> (Vec<Rate>, Vec<Rate>, Vec<Rate>) {
        (
            vec![rate(38.6245, Some("2026-01-01T00:00Z"), None)],
            vec![rate(6.5, Some("2026-01-01T00:00Z"), None)],
            vec![rate(51.94, Some("2026-01-01T00:00Z"), None)],
        )
    }

    /// IOG_ECO7_TOU, as returned by Kraken for IOG-SMB-VAR-26-10-29.
    fn iog_timeslots() -> Vec<Timeslot> {
        vec![
            Timeslot::new("ECO7_DAY", "Europe/London", "05:30:00", "23:30:00").unwrap(),
            Timeslot::new("ECO7_NIGHT", "Europe/London", "23:30:00", "05:30:00").unwrap(),
        ]
    }

    fn expand_iog(window: TimeRange, dispatches: &[TimeRange]) -> Vec<Slot> {
        let (day, night, standing) = iog_rates();
        expand_day_night(&day, &night, &iog_timeslots(), &standing, &window, dispatches).unwrap()
    }

    fn off_peak_starts(slots: &[Slot]) -> Vec<DateTime<Utc>> {
        slots.iter().filter(|s| s.value == 6.5).map(|s| s.valid_from).collect()
    }

    #[test]
    fn iog_off_peak_is_twelve_slots_from_2330_local() {
        let slots = expand_iog(range("2026-11-22T12:00Z", "2026-11-23T12:00Z"), &[]);

        let off_peak = off_peak_starts(&slots);
        assert_eq!(off_peak.len(), 12);
        assert_eq!(off_peak[0], dt("2026-11-22T23:30Z"), "GMT: local == UTC");
        assert_eq!(*off_peak.last().unwrap(), dt("2026-11-23T05:00Z"));
        assert!(slots.iter().all(|s| s.standing_charge == 51.94));
    }

    #[test]
    fn iog_off_peak_follows_local_time_in_summer() {
        let slots = expand_iog(range("2027-06-01T12:00Z", "2027-06-02T12:00Z"), &[]);

        let off_peak = off_peak_starts(&slots);
        assert_eq!(off_peak.len(), 12);
        assert_eq!(off_peak[0], dt("2027-06-01T22:30Z"), "BST: 23:30 local is 22:30Z");
        assert_eq!(*off_peak.last().unwrap(), dt("2027-06-02T04:00Z"));
    }

    #[test]
    fn iog_off_peak_on_dst_change_nights() {
        // Clocks go back at 02:00 BST on 2026-10-25: 23:30 BST to 05:30 GMT is 7 hours.
        let autumn = expand_iog(range("2026-10-24T12:00Z", "2026-10-25T12:00Z"), &[]);
        let off_peak = off_peak_starts(&autumn);
        assert_eq!(off_peak[0], dt("2026-10-24T22:30Z"));
        assert_eq!(*off_peak.last().unwrap(), dt("2026-10-25T05:00Z"));
        assert_eq!(off_peak.len(), 14);

        // Clocks go forward at 01:00 GMT on 2027-03-28: 23:30 GMT to 05:30 BST is 5 hours.
        let spring = expand_iog(range("2027-03-27T12:00Z", "2027-03-28T12:00Z"), &[]);
        let off_peak = off_peak_starts(&spring);
        assert_eq!(off_peak[0], dt("2027-03-27T23:30Z"));
        assert_eq!(*off_peak.last().unwrap(), dt("2027-03-28T04:00Z"));
        assert_eq!(off_peak.len(), 10);
    }

    #[test]
    fn dispatch_discounts_every_slot_it_touches() {
        let dispatches = [range("2026-11-22T14:10Z", "2026-11-22T15:05Z")];
        let slots = expand_iog(range("2026-11-22T13:30Z", "2026-11-22T16:00Z"), &dispatches);

        assert_eq!(
            off_peak_starts(&slots),
            [dt("2026-11-22T14:00Z"), dt("2026-11-22T14:30Z"), dt("2026-11-22T15:00Z")],
        );
    }

    #[test]
    fn dispatch_ending_on_a_slot_boundary_does_not_touch_the_next_slot() {
        // The first real dispatch, 2026-11-22.
        let dispatches = [range("2026-11-22T15:30Z", "2026-11-22T16:00Z")];
        let slots = expand_iog(range("2026-11-22T15:00Z", "2026-11-22T16:30Z"), &dispatches);
        assert_eq!(off_peak_starts(&slots), [dt("2026-11-22T15:30Z")]);
    }

    #[test]
    fn go_style_window_in_another_timezone() {
        // Windows aren't fixed to IOG's: e.g. 00:30-04:30 local, here in a zone without DST.
        let timeslots = [
            Timeslot::new("GO_NIGHT", "Etc/GMT-1", "00:30:00", "04:30:00").unwrap(),
            Timeslot::new("GO_DAY", "Etc/GMT-1", "04:30:00", "00:30:00").unwrap(),
        ];
        let (day, night, standing) = iog_rates();
        let slots = expand_day_night(&day, &night, &timeslots, &standing, &range("2027-06-01T12:00Z", "2027-06-02T12:00Z"), &[]).unwrap();
        let off_peak = off_peak_starts(&slots);
        assert_eq!(off_peak.len(), 8);
        assert_eq!(off_peak[0], dt("2027-06-01T23:30Z"));
    }

    #[test]
    fn uncovered_slot_is_an_error() {
        let timeslots = [Timeslot::new("ECO7_NIGHT", "Europe/London", "23:30:00", "05:30:00").unwrap()];
        let (day, night, standing) = iog_rates();
        let window = range("2026-11-22T04:00Z", "2026-11-22T07:00Z");
        assert!(expand_day_night(&day, &night, &timeslots, &standing, &window, &[]).is_err());
    }

    #[test]
    fn timeslot_names_map_to_day_or_night() {
        assert_eq!(Timeslot::new("ECO7_DAY", "Europe/London", "05:30:00", "23:30:00").unwrap().rate, TimeslotRate::Day);
        assert!(Timeslot::new("PEAK", "Europe/London", "16:00:00", "19:00:00").is_err());
        assert!(Timeslot::new("ECO7_DAY", "Mars/Olympus", "05:30:00", "23:30:00").is_err());
    }

    #[test]
    fn bump_charges_are_not_discounted() {
        assert!(is_discounted_dispatch(Some("smart-charge")));
        assert!(is_discounted_dispatch(None));
        assert!(!is_discounted_dispatch(Some("bump-charge")));
    }

    /// Time-of-use tariffs that publish their windows as standard unit rates,
    /// so they're priced by `expand_half_hours` without any timeslots. Fixtures
    /// are region A `standard-unit-rates` around the 2026 clock changes.
    mod published_windows {
        use super::*;
        use std::collections::BTreeMap;
        use chrono::NaiveDate;
        use crate::clients::octopus::TariffRates;

        const GO_SPRING: &str = include_str!("tariffs/fixtures/GO-VAR-22-10-14-A-spring-2026.json");
        const GO_AUTUMN: &str = include_str!("tariffs/fixtures/GO-VAR-22-10-14-A-autumn-2026.json");
        const COSY_SPRING: &str = include_str!("tariffs/fixtures/COSY-22-12-08-A-spring-2026.json");
        const COSY_AUTUMN: &str = include_str!("tariffs/fixtures/COSY-22-12-08-A-autumn-2026.json");
        const FLUX_IMPORT_SPRING: &str = include_str!("tariffs/fixtures/FLUX-IMPORT-23-02-14-A-spring-2026.json");
        const FLUX_IMPORT_AUTUMN: &str = include_str!("tariffs/fixtures/FLUX-IMPORT-23-02-14-A-autumn-2026.json");
        const FLUX_EXPORT_SPRING: &str = include_str!("tariffs/fixtures/FLUX-EXPORT-23-02-14-A-spring-2026.json");
        const FLUX_EXPORT_AUTUMN: &str = include_str!("tariffs/fixtures/FLUX-EXPORT-23-02-14-A-autumn-2026.json");

        const ALL: [&str; 8] = [
            GO_SPRING, GO_AUTUMN, COSY_SPRING, COSY_AUTUMN,
            FLUX_IMPORT_SPRING, FLUX_IMPORT_AUTUMN, FLUX_EXPORT_SPRING, FLUX_EXPORT_AUTUMN,
        ];

        fn parse(json: &str) -> TariffRates {
            serde_json::from_str(json).unwrap()
        }

        fn rates(json: &str) -> Vec<Rate> {
            parse(json).results.iter()
                .map(|r| Rate::from_api(r.value_inc_vat, Some(&r.valid_from), r.valid_to.as_deref()).unwrap())
                .collect()
        }

        /// The span the fixture covers, from its earliest to latest row.
        fn span(rates: &[Rate]) -> TimeRange {
            let start = rates.iter().filter_map(|r| r.valid_from).min().unwrap();
            let end = rates.iter().filter_map(|r| r.valid_to).max().unwrap();
            TimeRange::new(start, end).unwrap()
        }

        fn slots(json: &str) -> Vec<Slot> {
            let rates = rates(json);
            expand_half_hours(&rates, &[], &span(&rates))
        }

        /// Half-hours per price on one UK local day.
        fn local_day(slots: &[Slot], day: &str) -> BTreeMap<String, usize> {
            let day = NaiveDate::parse_from_str(day, "%Y-%m-%d").unwrap();
            let mut counts = BTreeMap::new();
            for slot in slots.iter().filter(|s| s.valid_from.with_timezone(&London).date_naive() == day) {
                *counts.entry(format!("{}", slot.value)).or_default() += 1;
            }
            counts
        }

        /// Prices as written in the API, rounded through f32 like `Rate::value`
        /// (e.g. 29.824725 is held as 29.824724).
        fn counts(pairs: &[(&str, usize)]) -> BTreeMap<String, usize> {
            pairs.iter()
                .map(|(price, n)| (format!("{}", price.parse::<f32>().unwrap()), *n))
                .collect()
        }

        #[test]
        fn windows_tile_without_gaps_or_overlaps() {
            for json in ALL {
                let rates = rates(json);
                let window = span(&rates);
                let slots = expand_half_hours(&rates, &[], &window);
                assert_eq!(slots.len(), half_hours(&window).count(), "every half-hour priced");
                for t in half_hours(&window) {
                    assert_eq!(rates.iter().filter(|r| r.covers(t)).count(), 1, "one window covers {t}");
                }
            }
        }

        #[test]
        fn rows_are_newest_first_and_bounded() {
            // Unlike day/night rates, every window row is closed.
            for json in ALL {
                let results = parse(json).results;
                assert!(results.first().unwrap().valid_from > results.last().unwrap().valid_from);
                assert!(results.iter().all(|r| r.valid_to.is_some() && r.payment_method.is_none()));
            }
        }

        #[test]
        fn import_rates_carried_vat_until_the_october_2026_holiday() {
            // Export (Flux Export) is never VAT-able.
            let ratios = |json: &str| -> Vec<f32> {
                let mut ratios: Vec<f32> = parse(json).results.iter()
                    .map(|r| (r.value_inc_vat / r.value_exc_vat * 100.0).round() / 100.0)
                    .collect();
                ratios.dedup();
                ratios
            };
            for json in [GO_SPRING, COSY_SPRING, FLUX_IMPORT_SPRING] {
                assert_eq!(ratios(json), [1.05]);
            }
            for json in [GO_AUTUMN, COSY_AUTUMN, FLUX_IMPORT_AUTUMN, FLUX_EXPORT_SPRING, FLUX_EXPORT_AUTUMN] {
                assert_eq!(ratios(json), [1.0]);
            }
        }

        #[test]
        fn go_cheap_window_is_0030_to_0530_local() {
            let slots = slots(GO_AUTUMN);
            let cheap: Vec<_> = slots.iter().filter(|s| s.value == 9.4).map(|s| s.valid_from).collect();
            // BST: 00:30 local is 23:30Z the previous UTC day.
            assert_eq!(cheap[0], dt("2026-10-22T23:30Z"));
            assert!(cheap.contains(&dt("2026-10-26T00:30Z")), "GMT: local == UTC");
            assert!(!cheap.contains(&dt("2026-10-26T05:30Z")));
        }

        #[test]
        fn go_clock_change_hour_falls_in_the_cheap_window() {
            assert_eq!(local_day(&slots(GO_AUTUMN), "2026-10-24"), counts(&[("9.4", 10), ("34.7089", 38)]));
            assert_eq!(local_day(&slots(GO_AUTUMN), "2026-10-25"), counts(&[("9.4", 12), ("34.7089", 38)]));
            assert_eq!(local_day(&slots(GO_SPRING), "2026-03-28"), counts(&[("8.49975", 10), ("30.605085", 38)]));
            assert_eq!(local_day(&slots(GO_SPRING), "2026-03-29"), counts(&[("8.49975", 8), ("30.605085", 38)]));
        }

        #[test]
        fn cosy_has_three_cheap_windows_and_a_peak() {
            // 04:00-07:00, 13:00-16:00 and 22:00-00:00 cheap; 16:00-19:00 peak.
            let slots = slots(COSY_AUTUMN);
            assert_eq!(
                local_day(&slots, "2026-10-26"),
                counts(&[("14.1309", 16), ("28.8151", 26), ("43.2226", 6)]),
            );
            let at = |t: &str| slots.iter().find(|s| s.valid_from == dt(t)).unwrap().value;
            assert_eq!(at("2026-10-26T03:30Z"), 28.8151);
            assert_eq!(at("2026-10-26T04:00Z"), 14.1309);
            assert_eq!(at("2026-10-26T16:00Z"), 43.2226);
            assert_eq!(at("2026-10-26T22:00Z"), 14.1309);
        }

        #[test]
        fn cosy_clock_change_hour_falls_in_a_standard_window() {
            // 00:00-04:00 absorbs it; cheap and peak time are unchanged.
            assert_eq!(
                local_day(&slots(COSY_AUTUMN), "2026-10-25"),
                counts(&[("14.1309", 16), ("28.8151", 28), ("43.2226", 6)]),
            );
            assert_eq!(
                local_day(&slots(COSY_SPRING), "2026-03-29"),
                counts(&[("14.62608", 16), ("29.824725", 24), ("44.737035", 6)]),
            );
        }

        #[test]
        fn flux_import_and_export_share_windows() {
            for (import, export) in [(FLUX_IMPORT_SPRING, FLUX_EXPORT_SPRING), (FLUX_IMPORT_AUTUMN, FLUX_EXPORT_AUTUMN)] {
                let bounds = |json| {
                    let mut b: Vec<_> = rates(json).iter().map(|r| (r.valid_from, r.valid_to)).collect();
                    b.sort();
                    b
                };
                assert_eq!(bounds(import), bounds(export));
            }
        }

        #[test]
        fn flux_cheap_0200_to_0500_and_peak_1600_to_1900_local() {
            let import = slots(FLUX_IMPORT_AUTUMN);
            let export = slots(FLUX_EXPORT_AUTUMN);
            let at = |slots: &[Slot], t: &str| slots.iter().find(|s| s.valid_from == dt(t)).unwrap().value;
            // BST
            assert_eq!(at(&import, "2026-10-24T01:00Z"), 16.1769);
            assert_eq!(at(&import, "2026-10-24T15:00Z"), 37.409);
            // GMT
            assert_eq!(at(&import, "2026-10-26T02:00Z"), 16.1769);
            assert_eq!(at(&export, "2026-10-26T02:00Z"), 4.7216);
            assert_eq!(at(&import, "2026-10-26T16:00Z"), 37.409);
            assert_eq!(at(&export, "2026-10-26T16:00Z"), 30.3317);
        }

        #[test]
        fn flux_clock_change_hour_falls_in_the_overnight_standard_window() {
            // 19:00-02:00 absorbs it, for import and export alike.
            assert_eq!(
                local_day(&slots(FLUX_IMPORT_AUTUMN), "2026-10-25"),
                counts(&[("16.1769", 6), ("26.793", 38), ("37.409", 6)]),
            );
            assert_eq!(
                local_day(&slots(FLUX_EXPORT_AUTUMN), "2026-10-25"),
                counts(&[("4.7216", 6), ("11.1216", 38), ("30.3317", 6)]),
            );
            assert_eq!(
                local_day(&slots(FLUX_IMPORT_SPRING), "2026-03-29"),
                counts(&[("17.18598", 6), ("28.64316", 34), ("40.100445", 6)]),
            );
        }
    }
}
