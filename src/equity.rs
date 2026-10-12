//! Equity holdings from a Carta capture.
//!
//! `browser/src/carta.ts` saves the responses of Carta's own read-only API
//! routes into one bundle file. This picks the holdings out of that bundle:
//! the companies, option grants with their vesting schedules, share
//! certificates, exercises and tender sales. Everything is returned as JSON
//! rows for `Database::replace_equity` to store.

use anyhow::{Context, Result};
use chrono::{DateTime, NaiveDate, Utc};
use serde_json::{json, Value};

/// Carta writes dates this way in its holdings tables
const TABLE_DATE_FORMAT: &str = "%m/%d/%Y";

/// The holdings found in one capture
#[derive(Debug, Default)]
pub struct Holdings {
    pub captured_at: Option<DateTime<Utc>>,
    pub issuers: Vec<Value>,
    pub grants: Vec<Value>,
    pub vesting_events: Vec<Value>,
    pub certificates: Vec<Value>,
    pub exercises: Vec<Value>,
    pub tenders: Vec<Value>,
}

/// A number Carta may send as a JSON number or as a decimal string
fn number(value: &Value) -> Option<f64> {
    match value {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

/// The amount of a `{"currency": "USD", "amount": "0.46"}` object
fn money(value: &Value) -> Option<f64> {
    number(&value["amount"])
}

/// A date from a millisecond timestamp, in UTC
fn date_from_millis(value: &Value) -> Option<String> {
    let at = DateTime::<Utc>::from_timestamp_millis(value.as_i64()?)?;
    Some(at.date_naive().to_string())
}

/// `03/31/2022` as `2022-03-31`
fn table_date(value: &Value) -> Option<String> {
    NaiveDate::parse_from_str(value.as_str()?, TABLE_DATE_FORMAT).ok().map(|d| d.to_string())
}

/// The date part of an ISO date or timestamp
fn iso_date(value: &Value) -> Option<String> {
    value.as_str()?.get(..10).map(str::to_string)
}

/// Read a bundle's holdings. Routes missing from the bundle, or that Carta
/// answered with an error, simply contribute nothing.
pub fn parse(bundle: &Value) -> Result<Holdings> {
    let portfolio = bundle["portfolio_id"].as_str().context("Bundle has no portfolio_id")?;
    let responses = bundle["responses"].as_object().context("Bundle has no responses")?;
    let body = |route: &str| -> &Value {
        match responses.get(route) {
            Some(captured) if captured["status"] == 200 => &captured["body"],
            _ => &Value::Null,
        }
    };

    let mut holdings = Holdings {
        captured_at: bundle["captured_at"].as_str().and_then(|at| at.parse().ok()),
        ..Holdings::default()
    };

    let companies = &body(&format!("/api/investors/portfolio/fund/{portfolio}/list/"))["results"]["companies"];
    for company in companies.as_array().into_iter().flatten() {
        let Some(issuer) = company["corporation_id"].as_i64() else {
            continue;
        };
        let base = format!("/api/investors/holdings/portfolio/{portfolio}/corporation/{issuer}");
        let dashboard = body(&format!("{base}/holdings-dashboard/"));
        let fmv = body(&format!("/api/issuers/v1/{issuer}/fair-market-value/portfolio/{portfolio}/"));
        holdings.issuers.push(json!({
            "issuer_id": issuer,
            "name": company["name"],
            "fmv": money(&fmv["fair_market_value"]["value"]),
            "held_since": iso_date(&dashboard["held_since"]),
            "cash_cost": number(&dashboard["cash_cost"]),
            "raw": company,
        }));

        let securities = body(&format!("{base}/securities/"));
        let option_rows = &body(&format!("{base}/options/"))["rows"];
        for grant in securities["option_grants"]["holdings"].as_array().into_iter().flatten() {
            let Some(id) = grant["id"].as_i64() else {
                continue;
            };
            // The options table carries what is exercisable right now
            let row = option_rows.as_array().into_iter().flatten().find(|row| row["id"] == grant["id"]);
            let vesting = body(&format!("/api/corporations/{issuer}/option_grant/{id}/vesting-data/"));
            let template = &vesting["vesting_manager"]["vesting_template"];
            let vested = grant["cumulative_vested_shares"].as_i64().unwrap_or(0);
            let exercised = grant["exercised_quantity"].as_i64().unwrap_or(0);
            let canceled = grant["is_canceled"].as_bool().unwrap_or(false);
            let exercisable = match row.and_then(|row| row["exercisable"].as_i64()) {
                Some(exercisable) => exercisable,
                None if canceled => 0,
                None => (vested - exercised).max(0),
            };
            holdings.grants.push(json!({
                "grant_id": id,
                "issuer_id": issuer,
                "label": grant["label"],
                "option_type": grant["sub_type"],
                "granted_as": row.map(|row| &row["issuable_type"]),
                "stock_type": row.map(|row| &row["stock_type"]),
                "issue_date": date_from_millis(&grant["issue_ts_ms"]),
                "expiration_date": date_from_millis(&grant["expiration_ts_ms"]),
                "quantity": grant["quantity"].as_i64().unwrap_or(0),
                "vested": vested,
                "exercised": exercised,
                "exercisable": exercisable,
                "exercise_price": money(&grant["exercise_price"]).unwrap_or(0.0),
                "status": grant["status"],
                "is_canceled": canceled,
                "vesting_start": iso_date(&vesting["vesting_start_date"]),
                "vesting_end": iso_date(&vesting["vesting_end_date"]),
                "vesting_schedule": vesting["vesting_manager"]["name"],
                "cliff": template["cliff_summary"],
                "vesting_terms": template["description"],
                "acceleration": vesting["vesting_manager"]["acceleration"],
                "early_exercise": vesting["exercise_data"]["early_exercise"],
                "iso_disqualified": row.map(|row| &row["is_nso_iso_disqualified"]),
                "form_83b_status": row.map(|row| &row["form_83b_to_file_status"]),
                "raw": grant,
            }));
            for event in vesting["vesting_event_data"].as_array().into_iter().flatten() {
                holdings.vesting_events.push(json!({
                    "grant_id": id,
                    "vest_date": iso_date(&event["date"]),
                    "quantity": number(&event["amount"]).unwrap_or(0.0) as i64,
                    "cumulative": number(&event["cumulative"]).unwrap_or(0.0) as i64,
                    "iso_quantity": event["iso_amount"].as_i64(),
                    "nso_quantity": event["nso_amount"].as_i64(),
                    "exercised": number(&event["progress_data"]["total_exercised"]).map(|n| n as i64),
                    "has_vested": event["has_vested"].as_bool().unwrap_or(false),
                    "kind": event["description"],
                }));
            }
        }

        // The shares table has cost and provenance; the securities list has
        // the fair market value on the day the shares were acquired
        let shares_route = responses.keys().find(|route| route.starts_with(&format!("{base}/shares/")));
        let share_classes = shares_route.map(|route| &body(route)["rows"]).unwrap_or(&Value::Null);
        for class in share_classes.as_array().into_iter().flatten() {
            for certificate in class["securities"].as_array().into_iter().flatten() {
                let Some(id) = certificate["id"].as_i64() else {
                    continue;
                };
                let listed = securities["certificates"]["holdings"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|listed| listed["id"] == certificate["id"]);
                holdings.certificates.push(json!({
                    "certificate_id": id,
                    "issuer_id": issuer,
                    "label": certificate["label"],
                    "share_class": class["name"],
                    "stock_type": certificate["stock_type"],
                    "is_certificated": certificate["is_certificated"],
                    "issue_date": table_date(&certificate["issue_date"]),
                    "acquired_date": table_date(&certificate["original_acquisition_date"]),
                    "quantity": certificate["quantity"].as_i64().unwrap_or(0),
                    "cost": number(&certificate["cost"]),
                    "status": certificate["status"],
                    "is_canceled": certificate["is_canceled"].as_bool().unwrap_or(false),
                    "exercise_type": certificate["exercise_type"],
                    "exercised_from": certificate["exercise_from"],
                    "fmv_at_acquisition": listed.and_then(|listed| money(&listed["fmv_at_exercise_date"])),
                    "raw": certificate,
                }));
            }
        }
    }

    let exercises = body(&format!("/api/exercise/v1/corporation/{portfolio}/exercise-history/"));
    for exercise in exercises.as_array().into_iter().flatten() {
        let Some(certificate) = exercise["certificate"]["id"].as_i64() else {
            continue;
        };
        let exercised_at = exercise["exercise_date"].as_i64().and_then(DateTime::<Utc>::from_timestamp_millis);
        holdings.exercises.push(json!({
            "certificate_id": certificate,
            "issuer_id": exercise["grant"]["issuer_id"],
            "grant_id": exercise["grant"]["id"],
            "grant_label": exercise["grant"]["label"],
            "certificate_label": exercise["certificate"]["label"],
            "option_type": exercise["grant"]["type"],
            "exercised_at": exercised_at.map(|at| at.to_rfc3339()),
            "quantity": number(&exercise["quantity"]).unwrap_or(0.0) as i64,
            "exercise_price": money(&exercise["exercise_price"]),
            "cost": money(&exercise["exercise_cost"]["total"]),
            "taxes": money(&exercise["exercise_cost"]["total_taxes"]),
            "early_exercise": exercise["is_early_exercise"],
            "fmv": money(&exercise["fair_market_value"]["value"]),
            "payment_method": exercise["payment_method"],
            "status": exercise["exercise_state"],
            "raw": exercise,
        }));
    }

    // Stored under its full URL, query string and all
    let tenders_route = format!("/corporations/{portfolio}/tender_offers/historical_tender_offers_api/");
    let tenders = responses.keys().find(|route| route.starts_with(&tenders_route));
    let tenders = tenders.map(|route| &body(route)["results"]).unwrap_or(&Value::Null);
    for tender in tenders.as_array().into_iter().flatten() {
        let Some(id) = tender["id"].as_i64() else {
            continue;
        };
        holdings.tenders.push(json!({
            "tender_id": id,
            "issuer_name": tender["corporation_name"],
            "name": tender["name"],
            "status": tender["status"],
            "kind": tender["tender_offer_type"],
            "start_date": iso_date(&tender["start_date"]),
            "end_date": iso_date(&tender["end_date"]),
            "settlement_date": iso_date(&tender["settlement_date"]),
            "price": number(&tender["price"]),
            "shares_sold": tender["shares_participated"].as_i64(),
            "proceeds": number(&tender["proceeds"]),
            "raw": tender,
        }));
    }

    Ok(holdings)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bundle() -> Value {
        json!({
            "captured_at": "2026-10-11T15:25:35.531Z",
            "portfolio_id": "7",
            "responses": {
                "/api/investors/portfolio/fund/7/list/": {"status": 200, "body": {"results": {"companies": [
                    {"corporation_id": 100, "name": "Acme, Inc."}
                ]}}},
                "/api/issuers/v1/100/fair-market-value/portfolio/7/": {"status": 200, "body": {
                    "fair_market_value": {"value": {"currency": "USD", "amount": "2.50"}}
                }},
                "/api/investors/holdings/portfolio/7/corporation/100/holdings-dashboard/": {"status": 200, "body": {
                    "held_since": "2022-03-31", "cash_cost": 460.0
                }},
                "/api/investors/holdings/portfolio/7/corporation/100/securities/": {"status": 200, "body": {
                    "option_grants": {"holdings": [{
                        "id": 1, "label": "ES-1", "sub_type": "ISO", "quantity": 4800,
                        "issue_ts_ms": 1654844400000i64, "expiration_ts_ms": 1970377200000i64,
                        "is_canceled": false, "cumulative_vested_shares": 2400, "exercised_quantity": 1000,
                        "status": "PARTIALLY_EXERCISED", "exercise_price": {"currency": "USD", "amount": "0.46"}
                    }]},
                    "certificates": {"holdings": [{"id": 9, "fmv_at_exercise_date": {"currency": "USD", "amount": "1.24"}}]}
                }},
                "/api/investors/holdings/portfolio/7/corporation/100/options/": {"status": 200, "body": {
                    "rows": [{"id": 1, "exercisable": 1400, "issuable_type": "ISO/NSO Split", "stock_type": "Common",
                              "is_nso_iso_disqualified": false, "form_83b_to_file_status": "NONE_ELIGIBLE"}]
                }},
                "/api/corporations/100/option_grant/1/vesting-data/": {"status": 200, "body": {
                    "vesting_start_date": "2022-06-10", "vesting_end_date": "2026-06-10",
                    "exercise_data": {"early_exercise": false},
                    "vesting_manager": {"name": "1/48 monthly, 1 year cliff", "acceleration": "",
                                        "vesting_template": {"cliff_summary": "Yes, 25% vests at 1 year"}},
                    "vesting_event_data": [
                        {"date": "2023-06-10", "amount": 1200, "cumulative": 1200, "has_vested": true,
                         "description": "cliff", "iso_amount": 1200, "nso_amount": 0,
                         "progress_data": {"total_exercised": 1000}},
                        {"date": "2027-01-10", "amount": 100, "cumulative": 1300, "has_vested": false,
                         "description": "interval"}
                    ]
                }},
                "/api/investors/holdings/portfolio/7/corporation/100/shares/?exclude_canceled=False": {"status": 200, "body": {
                    "rows": [{"name": "Class B Common", "securities": [{
                        "id": 9, "label": "CB-9", "issue_date": "12/18/2024", "original_acquisition_date": "12/18/2024",
                        "quantity": 1000, "cost": 460.0, "status": "Outstanding", "is_canceled": false,
                        "exercise_type": "ISO", "exercise_from": "ES-1", "stock_type": "Common", "is_certificated": true
                    }]}]
                }},
                "/api/exercise/v1/corporation/7/exercise-history/": {"status": 200, "body": [{
                    "grant": {"id": 1, "type": "ISO", "label": "ES-1", "issuer_id": 100},
                    "certificate": {"id": 9, "label": "CB-9"},
                    "exercise_date": 1734567033420i64, "quantity": "1000.000000000000",
                    "exercise_cost": {"total": {"currency": "USD", "amount": "460.00"},
                                      "total_taxes": {"currency": "USD", "amount": "0.00"}},
                    "exercise_price": {"currency": "USD", "amount": "0.46"},
                    "fair_market_value": {"value": {"currency": "USD", "amount": "1.24"}},
                    "payment_method": "ACH", "exercise_state": "EXERCISED", "is_early_exercise": false
                }]},
                "/corporations/7/tender_offers/historical_tender_offers_api/?page=1": {"status": 200, "body": {"results": [{
                    "id": 5, "corporation_name": "Acme, Inc.", "name": "Acme Q4 Tender", "status": "Closed & Settled",
                    "start_date": "2025-10-28T07:00:00+00:00", "end_date": "2025-11-25T22:00:00+00:00",
                    "settlement_date": "2025-12-08T08:00:00+00:00", "price": "2.43",
                    "proceeds": 2430.0, "shares_participated": 1000, "tender_offer_type": "secondary_sale"
                }]}},
                "/api/investors/holdings/portfolio/7/corporation/100/rsu/": {"status": 403, "body": null}
            }
        })
    }

    #[test]
    fn reads_every_kind_of_holding() {
        let holdings = parse(&bundle()).unwrap();
        assert_eq!(holdings.issuers.len(), 1);
        assert_eq!(holdings.issuers[0]["fmv"], 2.5);
        assert_eq!(holdings.issuers[0]["held_since"], "2022-03-31");

        let grant = &holdings.grants[0];
        assert_eq!(grant["label"], "ES-1");
        assert_eq!(grant["issue_date"], "2022-06-10");
        assert_eq!(grant["exercisable"], 1400);
        assert_eq!(grant["exercise_price"], 0.46);
        assert_eq!(grant["vesting_schedule"], "1/48 monthly, 1 year cliff");
        assert_eq!(grant["granted_as"], "ISO/NSO Split");
        assert_eq!(grant["cliff"], "Yes, 25% vests at 1 year");
        assert_eq!(grant["early_exercise"], false);
        assert_eq!(grant["form_83b_status"], "NONE_ELIGIBLE");

        assert_eq!(holdings.vesting_events.len(), 2);
        assert_eq!(holdings.vesting_events[0]["iso_quantity"], 1200);
        assert_eq!(holdings.vesting_events[0]["exercised"], 1000);
        assert!(holdings.vesting_events[1]["iso_quantity"].is_null());
        assert!(holdings.vesting_events[1]["exercised"].is_null());

        let certificate = &holdings.certificates[0];
        assert_eq!(certificate["issue_date"], "2024-12-18");
        assert_eq!(certificate["share_class"], "Class B Common");
        assert_eq!(certificate["fmv_at_acquisition"], 1.24);
        assert_eq!(certificate["is_certificated"], true);

        assert_eq!(holdings.exercises[0]["quantity"], 1000);
        assert_eq!(holdings.exercises[0]["cost"], 460.0);
        assert_eq!(holdings.exercises[0]["taxes"], 0.0);
        assert_eq!(holdings.tenders[0]["settlement_date"], "2025-12-08");
        assert_eq!(holdings.tenders[0]["shares_sold"], 1000);
        assert_eq!(holdings.tenders[0]["kind"], "secondary_sale");
    }

    #[test]
    fn a_grant_missing_from_the_options_table_is_exercisable_for_what_is_vested() {
        let mut bundle = bundle();
        bundle["responses"]["/api/investors/holdings/portfolio/7/corporation/100/options/"]["status"] = json!(500);
        let holdings = parse(&bundle).unwrap();
        assert_eq!(holdings.grants[0]["exercisable"], 1400);
    }

    #[test]
    fn a_bundle_with_nothing_in_it_yields_nothing() {
        let holdings = parse(&json!({"portfolio_id": "7", "responses": {}})).unwrap();
        assert!(holdings.issuers.is_empty() && holdings.grants.is_empty());
        assert!(parse(&json!({})).is_err());
    }
}
