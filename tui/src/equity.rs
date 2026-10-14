//! Private company equity: option grants, their vesting, and the shares held
//!
//! The figures come from a Carta capture stored by the scraper. Values here
//! are before tax: an option is worth its spread over the exercise price at
//! the company's latest fair market value, and a share is worth that value.

use chrono::{DateTime, Datelike, Months, NaiveDate, Utc};

/// Carta's status for a certificate canceled because it was cut into others
const SPLIT_STATUS: &str = "Transferred";
/// Carta's status for a certificate sold in a tender offer
const SOLD_STATUS: &str = "Secondary";

/// How far back a certificate's lineage is followed
const MAX_LINEAGE: usize = 8;

/// Months shares must be held past for a sale's gain to be long-term
const LONG_TERM_MONTHS: u32 = 12;
/// Months from the grant an ISO's shares must be held past to keep ISO treatment
const ISO_GRANT_MONTHS: u32 = 24;

#[derive(serde::Serialize, Debug, Clone)]
pub struct Issuer {
    pub id: i64,
    pub name: String,
    /// Latest fair market value per share (the 409A price), if Carta has one
    pub fmv: Option<f64>,
    pub as_of: Option<NaiveDate>,
}

#[derive(serde::Serialize, Debug, Clone, Default)]
pub struct Grant {
    pub id: i64,
    pub issuer_id: i64,
    pub label: String,
    /// ISO, NSO, or `ISO/NSO Split` for a grant over the yearly ISO limit
    pub option_type: String,
    /// The stock the options buy
    pub stock_type: String,
    pub status: String,
    pub expires: Option<NaiveDate>,
    /// Options are granted at the fair market value of the day, so the
    /// strike doubles as a record of the share price then
    pub issued: Option<NaiveDate>,
    pub quantity: f64,
    pub vested: f64,
    pub exercised: f64,
    /// Vested and not yet exercised
    pub exercisable: f64,
    pub exercise_price: f64,
    pub canceled: bool,
    pub vesting_start: Option<NaiveDate>,
    pub vesting_end: Option<NaiveDate>,
    pub schedule: String,
    pub cliff: String,
    /// The schedule spelled out as in the grant agreement
    pub terms: String,
    /// Empty when vesting does not accelerate
    pub acceleration: String,
    /// Whether unvested options may be exercised, where Carta says
    pub early_exercise: Option<bool>,
    /// ISOs that have lost their status
    pub iso_disqualified: bool,
    /// Carta's 83(b) filing status; `NONE_ELIGIBLE` when there is nothing to file
    pub form_83b: String,
}

impl Grant {
    pub fn unvested(&self) -> f64 {
        if self.canceled {
            0.0
        } else {
            (self.quantity - self.vested).max(0.0)
        }
    }
}

/// One tranche of a grant vesting
#[derive(serde::Serialize, Debug, Clone)]
pub struct VestEvent {
    pub grant_id: i64,
    pub date: NaiveDate,
    pub quantity: f64,
    /// How the tranche divides between ISO and NSO, where Carta says
    pub iso: Option<f64>,
    pub nso: Option<f64>,
}

#[derive(serde::Serialize, Debug, Clone, Default)]
pub struct Certificate {
    pub issuer_id: i64,
    pub label: String,
    pub share_class: String,
    /// Whether a share certificate was issued, where Carta says
    pub certificated: Option<bool>,
    /// When this certificate was issued; later than `acquired` for one cut
    /// from an earlier certificate
    pub issued: Option<NaiveDate>,
    pub acquired: Option<NaiveDate>,
    /// Fair market value per share on the day of issue
    pub fmv_at_issue: Option<f64>,
    pub quantity: f64,
    pub cost: Option<f64>,
    pub status: String,
    pub canceled: bool,
    /// For shares bought by exercising options: ISO or NSO, and the grant
    pub exercise_type: String,
    pub exercised_from: String,
}

#[derive(serde::Serialize, Debug, Clone, Default)]
pub struct Exercise {
    pub issuer_id: i64,
    pub grant_label: String,
    pub certificate_label: String,
    pub option_type: String,
    pub date: Option<NaiveDate>,
    pub quantity: f64,
    pub cost: f64,
    /// Tax withheld on top of the cost
    pub taxes: f64,
    /// ACH, ...
    pub payment: String,
    /// Fair market value per share on the day
    pub fmv: Option<f64>,
}

/// A sale of shares into a company tender offer
#[derive(serde::Serialize, Debug, Clone, Default)]
pub struct Tender {
    pub name: String,
    pub status: String,
    /// `secondary_sale`, ...
    pub kind: String,
    /// The days the offer could be accepted
    pub opened: Option<NaiveDate>,
    pub closed: Option<NaiveDate>,
    pub settled: Option<NaiveDate>,
    pub shares: f64,
    pub proceeds: f64,
}

/// A company's fair market value as one capture found it
#[derive(serde::Serialize, Debug, Clone, Copy)]
pub struct Snapshot {
    pub issuer_id: i64,
    pub date: NaiveDate,
    pub fmv: Option<f64>,
}

#[derive(serde::Serialize, Debug, Clone, Default)]
pub struct Equity {
    /// When Carta was last read
    pub captured_at: Option<DateTime<Utc>>,
    pub issuers: Vec<Issuer>,
    pub grants: Vec<Grant>,
    /// Oldest first
    pub vesting: Vec<VestEvent>,
    pub certificates: Vec<Certificate>,
    pub exercises: Vec<Exercise>,
    pub tenders: Vec<Tender>,
    /// One per company per capture, oldest first
    pub snapshots: Vec<Snapshot>,
}

/// Holdings added up at one share price
#[derive(serde::Serialize, Debug, Clone, Copy, Default, PartialEq)]
pub struct Summary {
    pub shares: f64,
    pub shares_value: f64,
    pub exercisable: f64,
    pub exercisable_value: f64,
    pub unvested: f64,
    pub unvested_value: f64,
    /// Cash needed to exercise everything that is vested
    pub cost_to_exercise: f64,
    /// The exercisable options that are ISOs, where the split is known
    pub exercisable_iso: Option<f64>,
}

impl Summary {
    /// What could be kept on leaving: shares, and vested options once exercised
    pub fn vested_value(&self) -> f64 {
        self.shares_value + self.exercisable_value
    }

    pub fn total_value(&self) -> f64 {
        self.vested_value() + self.unvested_value
    }
}

/// Options counted by how they are taxed
#[derive(serde::Serialize, Debug, Clone, Copy, Default, PartialEq)]
pub struct Split {
    pub iso: f64,
    pub nso: f64,
}

/// A grant's options at each stage, divided between ISO and NSO
#[derive(serde::Serialize, Debug, Clone, Copy, Default, PartialEq)]
pub struct GrantSplit {
    pub granted: Split,
    pub vested: Split,
    pub exercised: Split,
    pub exercisable: Split,
    pub unvested: Split,
}

/// Options vesting on one day, across grants
#[derive(serde::Serialize, Debug, Clone, PartialEq)]
pub struct Vesting {
    pub date: NaiveDate,
    pub quantity: f64,
    pub value: f64,
    pub iso: f64,
    pub nso: f64,
    /// Labels of the grants vesting that day
    pub grants: Vec<String>,
}

/// The record behind an `Event`, as an index into `Equity`'s lists
#[derive(serde::Serialize, Debug, Clone, Copy, PartialEq)]
pub enum Source {
    Exercise(usize),
    Tender(usize),
}

/// Something that happened to the holdings: an exercise or a sale
#[derive(serde::Serialize, Debug, Clone, PartialEq)]
pub struct Event {
    pub source: Source,
    pub date: Option<NaiveDate>,
    pub what: String,
    pub detail: String,
    pub shares: f64,
    /// Cash out (negative) for an exercise, in (positive) for a sale
    pub cash: f64,
}

impl Equity {
    pub fn is_empty(&self) -> bool {
        self.issuers.is_empty()
    }

    pub fn issuer(&self, id: i64) -> Option<&Issuer> {
        self.issuers.iter().find(|i| i.id == id)
    }

    /// The company whose holdings are still live: the one with the most
    /// options and shares outstanding
    pub fn primary(&self) -> Option<&Issuer> {
        let held = |issuer: &Issuer| -> f64 {
            let options: f64 = self.live_grants().filter(|g| g.issuer_id == issuer.id).map(|g| g.quantity).sum();
            let shares: f64 = self.held_certificates().filter(|c| c.issuer_id == issuer.id).map(|c| c.quantity).sum();
            options + shares
        };
        self.issuers.iter().max_by(|a, b| held(a).total_cmp(&held(b)))
    }

    pub fn live_grants(&self) -> impl Iterator<Item = &Grant> {
        self.grants.iter().filter(|g| !g.canceled)
    }

    pub fn held_certificates(&self) -> impl Iterator<Item = &Certificate> {
        self.certificates.iter().filter(|c| !c.canceled)
    }

    fn price(&self, issuer_id: i64, factor: f64) -> f64 {
        self.issuer(issuer_id).and_then(|i| i.fmv).unwrap_or(0.0) * factor
    }

    /// What one option of a grant is worth at `factor` times the current price
    fn spread(&self, grant: &Grant, factor: f64) -> f64 {
        (self.price(grant.issuer_id, factor) - grant.exercise_price).max(0.0)
    }

    /// Everything held, valued at `factor` times each company's current price
    pub fn summary(&self, factor: f64) -> Summary {
        let mut summary = Summary::default();
        for certificate in self.held_certificates() {
            summary.shares += certificate.quantity;
            summary.shares_value += certificate.quantity * self.price(certificate.issuer_id, factor);
        }
        let mut iso_known = true;
        let mut iso = 0.0;
        for grant in self.live_grants() {
            let spread = self.spread(grant, factor);
            summary.exercisable += grant.exercisable;
            summary.exercisable_value += grant.exercisable * spread;
            summary.unvested += grant.unvested();
            summary.unvested_value += grant.unvested() * spread;
            summary.cost_to_exercise += grant.exercisable * grant.exercise_price;
            match self.exercisable_iso(grant) {
                Some(quantity) => iso += quantity,
                None => iso_known = false,
            }
        }
        summary.exercisable_iso = iso_known.then_some(iso);
        summary
    }

    /// The exercises made from a grant, oldest first
    pub fn exercises_of<'a>(&'a self, grant: &'a Grant) -> impl Iterator<Item = &'a Exercise> {
        self.exercises.iter().filter(move |x| x.grant_label == grant.label && x.issuer_id == grant.issuer_id)
    }

    /// The tranches of a grant vested so far, by type. None when Carta does
    /// not give the split.
    fn vested_split(&self, grant: &Grant) -> Option<Split> {
        let mut vested = Split::default();
        let mut remaining = grant.vested;
        for event in self.vesting.iter().filter(|e| e.grant_id == grant.id) {
            if remaining <= 0.0 {
                break;
            }
            // Tranches vest in order, so the vested total says how far in we are
            let share = (remaining / event.quantity).min(1.0);
            vested.iso += event.iso? * share;
            vested.nso += event.nso.unwrap_or(0.0) * share;
            remaining -= event.quantity;
        }
        Some(vested)
    }

    fn exercised_split(&self, grant: &Grant) -> Split {
        let of_type = |kind: &str| -> f64 {
            self.exercises_of(grant).filter(|x| x.option_type == kind).map(|x| x.quantity).sum()
        };
        Split { iso: of_type("ISO"), nso: of_type("NSO") }
    }

    /// How many of a grant's exercisable options are ISOs: the ISO tranches
    /// vested so far, less the ISOs already exercised. None when Carta does
    /// not give the split.
    pub fn exercisable_iso(&self, grant: &Grant) -> Option<f64> {
        let vested = self.vested_split(grant)?;
        Some((vested.iso - self.exercised_split(grant).iso).clamp(0.0, grant.exercisable))
    }

    /// A grant's options at each stage by type. None when Carta does not
    /// divide its tranches.
    pub fn split(&self, grant: &Grant) -> Option<GrantSplit> {
        let mut granted = Split::default();
        let mut tranches = 0;
        for event in self.vesting.iter().filter(|e| e.grant_id == grant.id) {
            granted.iso += event.iso?;
            granted.nso += event.nso.unwrap_or(0.0);
            tranches += 1;
        }
        if tranches == 0 {
            return None;
        }
        let vested = self.vested_split(grant)?;
        let exercisable_iso = self.exercisable_iso(grant)?;
        Some(GrantSplit {
            granted,
            vested,
            exercised: self.exercised_split(grant),
            exercisable: Split { iso: exercisable_iso, nso: grant.exercisable - exercisable_iso },
            unvested: Split { iso: (granted.iso - vested.iso).max(0.0), nso: (granted.nso - vested.nso).max(0.0) },
        })
    }

    /// The tranches of a grant still to vest after `today`, soonest first
    pub fn vesting_ahead<'a>(&'a self, grant: &'a Grant, today: NaiveDate) -> impl Iterator<Item = &'a VestEvent> {
        self.vesting.iter().filter(move |e| e.grant_id == grant.id && e.date > today)
    }

    /// The certificate this one was cut from, when Carta canceled one
    /// certificate to issue several in its place (as when part of it is sold)
    pub fn parent(&self, certificate: &Certificate) -> Option<&Certificate> {
        let cut = |c: &Certificate| c.exercised_from.is_empty() && c.issued > c.acquired;
        if !cut(certificate) {
            return None;
        }
        let same_shares = |c: &&Certificate| c.issuer_id == certificate.issuer_id && c.acquired == certificate.acquired;
        let candidates: Vec<&Certificate> = self
            .certificates
            .iter()
            .filter(same_shares)
            .filter(|c| c.canceled && c.status == SPLIT_STATUS && c.issued < certificate.issued)
            .collect();
        if let [only] = candidates[..] {
            return Some(only);
        }
        // Several were cut that day: the pieces of one add up to it
        let pieces: f64 = self
            .certificates
            .iter()
            .filter(same_shares)
            .filter(|c| cut(c) && c.issued == certificate.issued)
            .map(|c| c.quantity)
            .sum();
        candidates.into_iter().find(|c| c.quantity == pieces)
    }

    /// The certificate these shares were first issued as
    pub fn origin<'a>(&'a self, certificate: &'a Certificate) -> &'a Certificate {
        let mut origin = certificate;
        for _ in 0..MAX_LINEAGE {
            match self.parent(origin) {
                Some(parent) => origin = parent,
                None => break,
            }
        }
        origin
    }

    /// The certificates this one was cut into
    pub fn pieces(&self, certificate: &Certificate) -> Vec<&Certificate> {
        self.certificates.iter().filter(|c| self.parent(c).is_some_and(|p| std::ptr::eq(p, certificate))).collect()
    }

    /// What a certificate's shares cost. Carta leaves the cost with the
    /// first certificate, so a piece cut from it takes its share of that.
    pub fn cost_of(&self, certificate: &Certificate) -> Option<f64> {
        let origin = self.origin(certificate);
        if std::ptr::eq(origin, certificate) || origin.quantity <= 0.0 {
            return certificate.cost;
        }
        origin.cost.map(|cost| cost * certificate.quantity / origin.quantity)
    }

    /// The exercise that bought a certificate's shares, if one did
    pub fn exercise_behind(&self, certificate: &Certificate) -> Option<&Exercise> {
        let origin = self.origin(certificate);
        self.exercises.iter().find(|x| x.certificate_label == origin.label && x.issuer_id == origin.issuer_id)
    }

    /// The grant a certificate's shares were exercised from, live or not
    pub fn grant_behind(&self, certificate: &Certificate) -> Option<&Grant> {
        let origin = self.origin(certificate);
        self.grants.iter().find(|g| g.label == origin.exercised_from && g.issuer_id == origin.issuer_id)
    }

    pub fn certificate(&self, issuer_id: i64, label: &str) -> Option<&Certificate> {
        self.certificates.iter().find(|c| c.issuer_id == issuer_id && c.label == label)
    }

    /// The tender a certificate was sold into: the first to settle once it existed
    pub fn tender_of(&self, certificate: &Certificate) -> Option<&Tender> {
        if !certificate.canceled || certificate.status != SOLD_STATUS {
            return None;
        }
        let issued = certificate.issued?;
        self.tenders.iter().filter(|t| t.settled.is_some_and(|d| d >= issued)).min_by_key(|t| t.settled)
    }

    /// The certificates sold into a tender
    pub fn sold_in(&self, tender: &Tender) -> Vec<&Certificate> {
        self.certificates.iter().filter(|c| self.tender_of(c).is_some_and(|t| std::ptr::eq(t, tender))).collect()
    }

    /// The first day a sale of a certificate's shares keeps ISO treatment (a
    /// qualifying disposition): more than a year after the exercise and more
    /// than two after the grant. None for shares that did not come from ISOs.
    pub fn iso_qualifying_from(&self, certificate: &Certificate) -> Option<NaiveDate> {
        let origin = self.origin(certificate);
        if origin.exercise_type != "ISO" {
            return None;
        }
        let granted = self.grant_behind(certificate)?.issued?;
        let after_grant = granted.checked_add_months(Months::new(ISO_GRANT_MONTHS))?.succ_opt()?;
        Some(after_grant.max(long_term_from(origin.acquired?)?))
    }

    /// Options still to vest after `today`, one entry per day, soonest first
    pub fn upcoming(&self, today: NaiveDate) -> Vec<Vesting> {
        let mut days: Vec<Vesting> = Vec::new();
        let mut events: Vec<&VestEvent> = self.vesting.iter().filter(|e| e.date > today).collect();
        events.sort_by_key(|e| e.date);
        for event in events {
            let Some(grant) = self.live_grants().find(|g| g.id == event.grant_id) else {
                continue;
            };
            let value = event.quantity * self.spread(grant, 1.0);
            if days.last().is_none_or(|day| day.date != event.date) {
                days.push(Vesting {
                    date: event.date,
                    quantity: 0.0,
                    value: 0.0,
                    iso: 0.0,
                    nso: 0.0,
                    grants: Vec::new(),
                });
            }
            let day = days.last_mut().expect("a day was just pushed");
            day.quantity += event.quantity;
            day.value += value;
            day.iso += event.iso.unwrap_or(0.0);
            day.nso += event.nso.unwrap_or(0.0);
            day.grants.push(grant.label.clone());
        }
        days
    }

    /// Value at today's price of the options vested and not yet exercised by
    /// each date, for live grants, oldest first: how it built up and what the
    /// schedule still holds
    pub fn vesting_curve(&self) -> Vec<(NaiveDate, f64)> {
        let mut events: Vec<(NaiveDate, f64)> = self
            .vesting
            .iter()
            .filter_map(|event| {
                let grant = self.live_grants().find(|g| g.id == event.grant_id)?;
                Some((event.date, event.quantity * self.spread(grant, 1.0)))
            })
            .collect();
        // Exercising turns options into shares, which are counted elsewhere
        events.extend(self.exercises.iter().filter_map(|exercise| {
            let grant =
                self.live_grants().find(|g| g.label == exercise.grant_label && g.issuer_id == exercise.issuer_id)?;
            Some((exercise.date?, -exercise.quantity * self.spread(grant, 1.0)))
        }));
        events.sort_by_key(|e| e.0);
        let mut total = 0.0;
        let mut curve: Vec<(NaiveDate, f64)> = Vec::new();
        for (date, value) in events {
            total += value;
            match curve.last_mut() {
                Some(last) if last.0 == date => last.1 = total,
                _ => curve.push((date, total)),
            }
        }
        curve
    }

    /// The share price over time, as far as the records show it, oldest
    /// first. Each price holds until the next one: the company revalues about
    /// yearly, and what is recorded is the price on the days something
    /// happened (a grant, an exercise, a sale, a capture).
    pub fn price_history(&self) -> Vec<(NaiveDate, f64)> {
        let Some(issuer) = self.primary() else {
            return Vec::new();
        };
        let mut seen: Vec<(NaiveDate, f64)> = Vec::new();
        let mut note = |date: Option<NaiveDate>, price: Option<f64>| {
            if let (Some(date), Some(price)) = (date, price) {
                if price > 0.0 {
                    seen.push((date, price));
                }
            }
        };
        for grant in self.grants.iter().filter(|g| g.issuer_id == issuer.id) {
            note(grant.issued, Some(grant.exercise_price));
        }
        for certificate in self.certificates.iter().filter(|c| c.issuer_id == issuer.id) {
            note(certificate.issued, certificate.fmv_at_issue);
        }
        for exercise in self.exercises.iter().filter(|x| x.issuer_id == issuer.id) {
            note(exercise.date, exercise.fmv);
        }
        for snapshot in self.snapshots.iter().filter(|s| s.issuer_id == issuer.id) {
            note(Some(snapshot.date), snapshot.fmv);
        }
        note(issuer.as_of, issuer.fmv);
        // Later sources are the better record of a day they share
        seen.sort_by_key(|p| p.0);
        let mut history: Vec<(NaiveDate, f64)> = Vec::new();
        for (date, price) in seen {
            match history.last_mut() {
                Some(last) if last.0 == date => last.1 = price,
                Some(last) if last.1 == price => {}
                _ => history.push((date, price)),
            }
        }
        history
    }

    /// When a certificate no longer held left: the day it was cut into
    /// others, or the tender it was sold into. None while it is still held.
    fn held_until(&self, certificate: &Certificate) -> Option<NaiveDate> {
        if !certificate.canceled {
            return None;
        }
        let issued = certificate.issued?;
        let split = self
            .certificates
            .iter()
            .filter(|c| c.issuer_id == certificate.issuer_id && c.acquired == certificate.acquired)
            .filter_map(|c| c.issued)
            .filter(|d| *d > issued)
            .min();
        let sold = self.tenders.iter().filter_map(|t| t.settled).filter(|d| *d >= issued).min();
        // With neither on record there is no telling how long it was held
        Some(split.or(sold).unwrap_or(issued))
    }

    /// What was vested (shares plus exercisable options at their spread) on
    /// each day something changed, at the share price of the time, oldest
    /// first and ending today. Drawn as steps: each value holds to the next.
    pub fn value_history(&self, today: NaiveDate) -> Vec<(NaiveDate, f64)> {
        let Some(issuer) = self.primary() else {
            return Vec::new();
        };
        let prices = self.price_history();
        let certificates: Vec<&Certificate> = self.certificates.iter().filter(|c| c.issuer_id == issuer.id).collect();
        let grants: Vec<&Grant> = self.live_grants().filter(|g| g.issuer_id == issuer.id).collect();
        let exercises: Vec<&Exercise> = self.exercises.iter().filter(|x| x.issuer_id == issuer.id).collect();

        let mut dates: Vec<NaiveDate> = prices.iter().map(|p| p.0).collect();
        dates.extend(certificates.iter().filter_map(|c| c.issued));
        dates.extend(certificates.iter().filter_map(|c| self.held_until(c)));
        dates.extend(exercises.iter().filter_map(|x| x.date));
        dates.extend(self.vesting.iter().filter(|e| grants.iter().any(|g| g.id == e.grant_id)).map(|e| e.date));
        dates.push(today);
        dates.retain(|d| *d <= today);
        dates.sort();
        dates.dedup();

        let value_on = |date: NaiveDate| -> f64 {
            let price = prices.iter().rev().find(|p| p.0 <= date).map(|p| p.1).unwrap_or(0.0);
            let shares: f64 = certificates
                .iter()
                .filter(|c| c.issued.is_some_and(|d| d <= date) && self.held_until(c).is_none_or(|end| end > date))
                .map(|c| c.quantity)
                .sum();
            let options: f64 = grants
                .iter()
                .map(|grant| {
                    let vested: f64 = self
                        .vesting
                        .iter()
                        .filter(|e| e.grant_id == grant.id && e.date <= date)
                        .map(|e| e.quantity)
                        .sum();
                    let exercised: f64 = exercises
                        .iter()
                        .filter(|x| x.grant_label == grant.label && x.date.is_some_and(|d| d <= date))
                        .map(|x| x.quantity)
                        .sum();
                    (vested - exercised).max(0.0) * (price - grant.exercise_price).max(0.0)
                })
                .sum();
            shares * price + options
        };

        let mut history: Vec<(NaiveDate, f64)> = Vec::new();
        for date in dates {
            let value = value_on(date);
            if let (Some(previous), Some(day_before)) = (history.last().copied(), date.pred_opt()) {
                // Hold the earlier value up to the eve of the change
                if day_before > previous.0 && previous.1 != value {
                    history.push((day_before, previous.1));
                }
            }
            history.push((date, value));
        }
        // Today is the holdings as Carta states them, not as reconstructed
        if let Some(last) = history.last_mut() {
            last.1 = self.summary(1.0).vested_value();
        }
        history
    }

    /// Exercises and sales, newest first
    pub fn history(&self) -> Vec<Event> {
        let mut events: Vec<Event> = Vec::new();
        for (index, exercise) in self.exercises.iter().enumerate() {
            let company = self.issuer(exercise.issuer_id).map(|i| i.name.as_str()).unwrap_or("");
            let fmv = exercise.fmv.map(|fmv| format!(" when worth ${:.2}", fmv)).unwrap_or_default();
            events.push(Event {
                source: Source::Exercise(index),
                date: exercise.date,
                what: format!("Exercised {} {}", exercise.grant_label, exercise.option_type),
                detail: format!("{} into {}{}", company, exercise.certificate_label, fmv),
                shares: exercise.quantity,
                cash: -exercise.cost,
            });
        }
        for (index, tender) in self.tenders.iter().enumerate() {
            let price = if tender.shares > 0.0 { tender.proceeds / tender.shares } else { 0.0 };
            events.push(Event {
                source: Source::Tender(index),
                date: tender.settled,
                what: "Sold in tender".to_string(),
                detail: format!("{} at ${:.3}", tender.name, price),
                shares: tender.shares,
                cash: tender.proceeds,
            });
        }
        events.sort_by(|a, b| b.date.cmp(&a.date).then_with(|| a.what.cmp(&b.what)));
        events
    }
}

/// The first day a sale of shares acquired on `acquired` is long-term: held
/// for more than a year
pub fn long_term_from(acquired: NaiveDate) -> Option<NaiveDate> {
    acquired.checked_add_months(Months::new(LONG_TERM_MONTHS))?.succ_opt()
}

/// `1y 9m` between two dates, for how long shares have been held
pub fn held_for(from: NaiveDate, today: NaiveDate) -> String {
    let months = (today.year() - from.year()) * 12 + today.month() as i32 - from.month() as i32
        - i32::from(today.day() < from.day());
    let months = months.max(0);
    match (months / 12, months % 12) {
        (0, months) => format!("{}m", months),
        (years, 0) => format!("{}y", years),
        (years, months) => format!("{}y {}m", years, months),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn grant(id: i64, quantity: f64, vested: f64, exercised: f64, price: f64) -> Grant {
        Grant {
            id,
            issuer_id: 1,
            label: format!("ES-{}", id),
            quantity,
            vested,
            exercised,
            exercisable: vested - exercised,
            exercise_price: price,
            ..Grant::default()
        }
    }

    fn equity() -> Equity {
        let event = |grant_id, d: NaiveDate, quantity, iso, nso| VestEvent {
            grant_id,
            date: d,
            quantity,
            iso: Some(iso),
            nso: Some(nso),
        };
        Equity {
            captured_at: None,
            issuers: vec![Issuer { id: 1, name: "Acme".into(), fmv: Some(2.0), as_of: None }],
            grants: vec![grant(10, 400.0, 300.0, 100.0, 0.5), grant(11, 200.0, 0.0, 0.0, 2.0)],
            vesting: vec![
                event(10, date(2025, 1, 1), 100.0, 100.0, 0.0),
                event(10, date(2026, 1, 1), 100.0, 40.0, 60.0),
                event(10, date(2026, 6, 1), 100.0, 0.0, 100.0),
                event(10, date(2027, 1, 1), 100.0, 0.0, 100.0),
                event(11, date(2027, 1, 1), 200.0, 200.0, 0.0),
            ],
            certificates: vec![
                Certificate {
                    issuer_id: 1,
                    label: "CB-1".into(),
                    issued: Some(date(2025, 2, 1)),
                    acquired: Some(date(2025, 2, 1)),
                    fmv_at_issue: Some(1.0),
                    quantity: 100.0,
                    cost: Some(50.0),
                    status: "Outstanding".into(),
                    exercise_type: "ISO".into(),
                    exercised_from: "ES-10".into(),
                    ..Certificate::default()
                },
                Certificate {
                    issuer_id: 1,
                    label: "CB-0".into(),
                    issued: Some(date(2024, 6, 1)),
                    acquired: Some(date(2024, 6, 1)),
                    fmv_at_issue: Some(0.5),
                    quantity: 900.0,
                    cost: None,
                    status: "Secondary".into(),
                    canceled: true,
                    ..Certificate::default()
                },
            ],
            exercises: vec![Exercise {
                issuer_id: 1,
                grant_label: "ES-10".into(),
                certificate_label: "CB-1".into(),
                option_type: "ISO".into(),
                date: Some(date(2025, 2, 1)),
                quantity: 100.0,
                cost: 50.0,
                fmv: Some(1.0),
                ..Exercise::default()
            }],
            tenders: vec![Tender {
                name: "Q4 Tender".into(),
                settled: Some(date(2025, 12, 8)),
                shares: 900.0,
                proceeds: 1800.0,
                ..Tender::default()
            }],
            snapshots: Vec::new(),
        }
    }

    #[test]
    fn summary_values_options_at_their_spread_and_shares_in_full() {
        let summary = equity().summary(1.0);
        assert_eq!(summary.shares, 100.0);
        assert_eq!(summary.shares_value, 200.0);
        assert_eq!(summary.exercisable, 200.0);
        assert_eq!(summary.exercisable_value, 300.0);
        // The second grant is at the money: unvested but worth nothing yet
        assert_eq!(summary.unvested, 300.0);
        assert_eq!(summary.unvested_value, 150.0);
        assert_eq!(summary.cost_to_exercise, 100.0);
        assert_eq!(summary.total_value(), 650.0);
    }

    #[test]
    fn a_higher_price_lifts_options_by_more_than_shares() {
        let equity = equity();
        let doubled = equity.summary(2.0);
        assert_eq!(doubled.shares_value, 400.0);
        assert_eq!(doubled.exercisable_value, 700.0);
        assert_eq!(doubled.unvested_value, 350.0 + 400.0);
    }

    #[test]
    fn exercisable_isos_are_vested_iso_tranches_less_isos_exercised() {
        let equity = equity();
        // Vested ISO tranches 100 + 40, of which 100 were exercised
        assert_eq!(equity.exercisable_iso(&equity.grants[0]), Some(40.0));
        assert_eq!(equity.summary(1.0).exercisable_iso, Some(40.0));
    }

    #[test]
    fn a_grant_divides_into_iso_and_nso_at_each_stage() {
        let equity = equity();
        let split = equity.split(&equity.grants[0]).unwrap();
        assert_eq!(split.granted, Split { iso: 140.0, nso: 260.0 });
        assert_eq!(split.vested, Split { iso: 140.0, nso: 160.0 });
        assert_eq!(split.exercised, Split { iso: 100.0, nso: 0.0 });
        assert_eq!(split.exercisable, Split { iso: 40.0, nso: 160.0 });
        assert_eq!(split.unvested, Split { iso: 0.0, nso: 100.0 });
        // Without Carta's division of the tranches there is nothing to say
        let mut undivided = equity.clone();
        undivided.vesting.iter_mut().for_each(|e| e.iso = None);
        assert_eq!(undivided.split(&undivided.grants[0]), None);
    }

    /// The fixture's ISO shares, later cut in two with one piece sold
    fn cut_and_sold() -> Equity {
        let mut equity = equity();
        equity.grants[0].issued = Some(date(2024, 1, 1));
        equity.certificates[0].status = "Transferred".into();
        equity.certificates[0].canceled = true;
        let piece = |label: &str, quantity, status: &str, canceled| Certificate {
            issuer_id: 1,
            label: label.into(),
            issued: Some(date(2025, 12, 8)),
            acquired: Some(date(2025, 2, 1)),
            quantity,
            cost: Some(0.0),
            status: status.into(),
            canceled,
            ..Certificate::default()
        };
        equity.certificates.push(piece("CB-2", 30.0, "Secondary", true));
        equity.certificates.push(piece("CB-3", 70.0, "Outstanding", false));
        equity
    }

    #[test]
    fn a_piece_of_a_cut_certificate_keeps_where_its_shares_came_from() {
        let equity = cut_and_sold();
        let kept = equity.certificate(1, "CB-3").unwrap();
        assert_eq!(equity.parent(kept).unwrap().label, "CB-1");
        assert_eq!(equity.origin(kept).exercise_type, "ISO");
        assert_eq!(equity.cost_of(kept), Some(35.0));
        assert_eq!(equity.exercise_behind(kept).unwrap().certificate_label, "CB-1");
        assert_eq!(equity.grant_behind(kept).unwrap().label, "ES-10");
        let whole = equity.certificate(1, "CB-1").unwrap();
        assert_eq!(equity.pieces(whole).iter().map(|c| c.label.as_str()).collect::<Vec<_>>(), vec!["CB-2", "CB-3"]);
        // Shares issued whole are their own origin
        assert!(equity.parent(whole).is_none());
        assert_eq!(equity.cost_of(whole), Some(50.0));
    }

    #[test]
    fn a_tender_knows_the_certificates_sold_into_it() {
        let equity = cut_and_sold();
        let sold: Vec<&str> = equity.sold_in(&equity.tenders[0]).iter().map(|c| c.label.as_str()).collect();
        assert_eq!(sold, vec!["CB-0", "CB-2"]);
        assert!(equity.tender_of(equity.certificate(1, "CB-3").unwrap()).is_none());
    }

    #[test]
    fn iso_shares_qualify_a_year_after_exercise_and_two_after_the_grant() {
        let mut equity = cut_and_sold();
        let kept = equity.certificate(1, "CB-3").unwrap();
        // Granted Jan 1 2024, exercised Feb 1 2025: the grant date binds
        assert_eq!(equity.iso_qualifying_from(kept), Some(date(2026, 2, 2)));
        equity.grants[0].issued = Some(date(2025, 1, 1));
        let kept = equity.certificate(1, "CB-3").unwrap();
        assert_eq!(equity.iso_qualifying_from(kept), Some(date(2027, 1, 2)));
        assert_eq!(equity.iso_qualifying_from(&equity.certificates[1]), None);
        assert_eq!(long_term_from(date(2024, 2, 29)), Some(date(2025, 3, 1)));
    }

    #[test]
    fn upcoming_groups_grants_vesting_the_same_day() {
        let upcoming = equity().upcoming(date(2026, 10, 11));
        assert_eq!(upcoming.len(), 1);
        assert_eq!(upcoming[0].quantity, 300.0);
        assert_eq!(upcoming[0].value, 150.0);
        assert_eq!(upcoming[0].grants, vec!["ES-10", "ES-11"]);
        assert_eq!((upcoming[0].iso, upcoming[0].nso), (200.0, 100.0));
    }

    #[test]
    fn vesting_curve_ends_at_what_is_exercisable_plus_what_will_vest() {
        let equity = equity();
        let curve = equity.vesting_curve();
        let at = |d: NaiveDate| curve.iter().rev().find(|p| p.0 <= d).map(|p| p.1).unwrap_or(0.0);
        // 100 vested at a 1.50 spread, then 100 exercised a month later
        assert_eq!(at(date(2025, 1, 15)), 150.0);
        assert_eq!(at(date(2025, 2, 15)), 0.0);
        assert_eq!(at(date(2026, 10, 11)), equity.summary(1.0).exercisable_value);
        assert_eq!(curve.last().unwrap().1, 450.0);
    }

    #[test]
    fn price_history_comes_from_the_days_something_was_recorded() {
        let mut equity = equity();
        equity.issuers[0].as_of = Some(date(2026, 10, 11));
        assert_eq!(
            equity.price_history(),
            vec![(date(2024, 6, 1), 0.5), (date(2025, 2, 1), 1.0), (date(2026, 10, 11), 2.0)]
        );
    }

    #[test]
    fn value_history_follows_shares_options_and_price_through_time() {
        let mut equity = equity();
        equity.issuers[0].as_of = Some(date(2026, 10, 11));
        let today = date(2026, 10, 11);
        let history = equity.value_history(today);
        let at = |d: NaiveDate| history.iter().rev().find(|p| p.0 <= d).map(|p| p.1).unwrap_or(0.0);
        // 900 shares at 0.50, before anything vested
        assert_eq!(at(date(2024, 7, 1)), 450.0);
        // 100 options vest at a strike equal to the price: still 450
        assert_eq!(at(date(2025, 1, 15)), 450.0);
        // Exercised into 100 shares as the price reaches 1.00
        assert_eq!(at(date(2025, 3, 1)), 1000.0);
        // The 900 are sold in the tender; 100 shares remain
        assert_eq!(at(date(2025, 12, 31)), 100.0);
        // Today matches the stated holdings
        assert_eq!(history.last().unwrap(), &(today, equity.summary(1.0).vested_value()));
    }

    #[test]
    fn history_lists_sales_and_exercises_newest_first() {
        let history = equity().history();
        assert_eq!(history[0].what, "Sold in tender");
        assert_eq!(history[0].source, Source::Tender(0));
        assert_eq!(history[1].source, Source::Exercise(0));
        assert_eq!(history[0].cash, 1800.0);
        assert_eq!(history[1].cash, -50.0);
    }

    #[test]
    fn holding_period_counts_whole_months() {
        assert_eq!(held_for(date(2024, 12, 18), date(2026, 10, 11)), "1y 9m");
        assert_eq!(held_for(date(2026, 9, 20), date(2026, 10, 11)), "0m");
        assert_eq!(held_for(date(2024, 10, 11), date(2026, 10, 11)), "2y");
    }
}
