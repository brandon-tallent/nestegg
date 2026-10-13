//! A household that does not exist, for screenshots and for trying the
//! interface without a database (`--demo`).
//!
//! Nothing here comes from anyone's records: the accounts, merchants and
//! funds are written out below, and the amounts are drawn from a fixed seed.
//! It is two salaries, a mortgage, a car loan, two credit cards paid in full
//! each month, retirement and brokerage accounts, and options in a private
//! company, lived forward a day at a time so that every balance follows from
//! the transactions before it. The history runs up to today, so bills are
//! due and charts are current whenever it is looked at, and what happened on
//! a given day is the same from one run to the next.

use crate::anonymize::equity;
use crate::data::{
    month_of, Account, Category, CategoryGroup, Data, Holding, NetWorthPoint, Portfolio, Position, Txn, ONE_TIME_TAG,
};
use crate::dice::{hash, Dice};
use crate::forecast::month_end;
use chrono::{Datelike, Duration, Months, NaiveDate, Utc, Weekday};
use serde_json::{json, Value};
use std::collections::HashMap;

/// What every draw starts from: another number moves a different household in
const SEED: u64 = 28;
/// Months of history before today
const HISTORY_MONTHS: u32 = 30;

const MONTHS_PER_YEAR: f64 = 12.0;
const PERCENT: f64 = 100.0;
const CENTS_PER_DOLLAR: f64 = 100.0;
const DAYS_PER_PAY_PERIOD: i64 = 14;

struct AccountSpec {
    id: &'static str,
    name: &'static str,
    mask: &'static str,
    /// Monarch's account type: depository, credit, brokerage, loan, ...
    kind: &'static str,
    subtype: &'static str,
    institution: &'static str,
    /// Signed balance when the history starts: negative for what is owed
    opening: f64,
    credit_limit: Option<f64>,
    /// Annual rate in percent, and the monthly payment of principal and interest
    rate: Option<f64>,
    payment: Option<f64>,
}

const fn account(
    id: &'static str,
    name: &'static str,
    mask: &'static str,
    kind: &'static str,
    subtype: &'static str,
    institution: &'static str,
    opening: f64,
) -> AccountSpec {
    AccountSpec { id, name, mask, kind, subtype, institution, opening, credit_limit: None, rate: None, payment: None }
}

impl AccountSpec {
    const fn limit(mut self, limit: f64) -> Self {
        self.credit_limit = Some(limit);
        self
    }

    const fn loan(mut self, rate: f64, payment: f64) -> Self {
        self.rate = Some(rate);
        self.payment = Some(payment);
        self
    }

    fn kind_display(&self) -> &'static str {
        match self.kind {
            "depository" => "Cash",
            "credit" => "Credit Cards",
            "brokerage" => "Investments",
            "loan" => "Loans",
            "real_estate" => "Real Estate",
            _ => "Equity",
        }
    }

    fn is_asset(&self) -> bool {
        !matches!(self.kind, "credit" | "loan")
    }
}

const CHECKING: usize = 0;
const SAVINGS: usize = 1;
const TRAVEL_CARD: usize = 2;
const CASH_CARD: usize = 3;
const RETIREMENT: usize = 4;
const ROTH: usize = 5;
const BROKERAGE: usize = 6;
const COLLEGE: usize = 7;
const MORTGAGE: usize = 8;
const AUTO_LOAN: usize = 9;
const HOME: usize = 10;
const EQUITY: usize = 11;

const ACCOUNTS: [AccountSpec; 12] = [
    account("checking", "Everyday Checking", "4417", "depository", "Checking", "First Harbor Bank", 12_000.0),
    account("savings", "High-Yield Savings", "8025", "depository", "Savings", "Lakeshore Bank", 16_500.0),
    account("travel-card", "Travel Rewards Card", "3391", "credit", "Credit Card", "Summit Trust", -1_840.0)
        .limit(18_000.0),
    account("cash-card", "Cash Back Card", "7706", "credit", "Credit Card", "Meridian Bank", -960.0).limit(9_500.0),
    account("retirement", "Larkspur 401(k)", "2254", "brokerage", "401k", "Northfield", 162_000.0),
    account("roth", "Roth IRA", "6138", "brokerage", "Roth IRA", "Granite Investments", 38_500.0),
    account("brokerage", "Joint Brokerage", "9470", "brokerage", "Brokerage", "Granite Investments", 54_000.0),
    account("college", "College Savings", "1183", "brokerage", "529 Plan", "Lakeshore 529", 14_200.0),
    account("mortgage", "Home Mortgage", "5562", "loan", "Mortgage", "Harborview Home Loans", -438_600.0)
        .loan(5.625, 2_560.0),
    account("auto-loan", "Auto Loan", "0914", "loan", "Auto", "Summit Auto Finance", -26_800.0).loan(5.4, 465.0),
    account("home", "Maple Street House", "", "real_estate", "Primary Home", "", 668_000.0),
    account("equity", "Larkspur Robotics Options", "", "equity", "Stock Options", "", 0.0),
];

/// Yearly interest the savings account pays, in percent
const SAVINGS_RATE: f64 = 4.1;

/// The private company the options are in, and where one of the salaries comes from
const COMPANY: &str = "Larkspur Robotics";
const OPTION_GRANTS: i64 = 4;
/// About what the vested options and shares come to
const EQUITY_WORTH: f64 = 185_000.0;

const INCOME: &str = "income";
const EXPENSE: &str = "expense";
const TRANSFER: &str = "transfer";

/// Category groups in display order, as (name, kind)
const GROUPS: [(&str, &str); 12] = [
    ("Income", INCOME),
    ("Housing", EXPENSE),
    ("Bills & Utilities", EXPENSE),
    ("Food & Dining", EXPENSE),
    ("Auto & Transport", EXPENSE),
    ("Children", EXPENSE),
    ("Shopping", EXPENSE),
    ("Travel & Lifestyle", EXPENSE),
    ("Health & Wellness", EXPENSE),
    ("Gifts & Donations", EXPENSE),
    ("Financial", EXPENSE),
    ("Transfers", TRANSFER),
];

struct CategorySpec {
    name: &'static str,
    group: &'static str,
    /// What the budget plans for it each month
    planned: f64,
    /// A bill of a set size, as against spending that is chosen
    fixed: bool,
}

const fn category(name: &'static str, group: &'static str, planned: f64, fixed: bool) -> CategorySpec {
    CategorySpec { name, group, planned, fixed }
}

const PAYCHECKS: &str = "Paychecks";
const INTEREST: &str = "Interest";
const DIVIDENDS: &str = "Dividends & Capital Gains";
const OTHER_INCOME: &str = "Other Income";
const MORTGAGE_PAYMENT: &str = "Mortgage";
const HOME_IMPROVEMENT: &str = "Home Improvement";
const RESTAURANTS: &str = "Restaurants & Bars";
const AUTO_PAYMENT: &str = "Auto Payment";
const SHOPPING: &str = "Shopping";
const TRAVEL: &str = "Travel & Vacation";
const ENTERTAINMENT: &str = "Entertainment";
const SUBSCRIPTIONS: &str = "Subscriptions";
const PLAIN_TRANSFER: &str = "Transfer";
const CARD_PAYMENT: &str = "Credit Card Payment";

const CATEGORIES: [CategorySpec; 34] = [
    category(PAYCHECKS, "Income", 14_700.0, true),
    category(INTEREST, "Income", 180.0, false),
    category(DIVIDENDS, "Income", 0.0, false),
    category(OTHER_INCOME, "Income", 0.0, false),
    category(MORTGAGE_PAYMENT, "Housing", 3_120.0, true),
    category(HOME_IMPROVEMENT, "Housing", 150.0, false),
    category("Gas & Electric", "Bills & Utilities", 150.0, true),
    category("Water", "Bills & Utilities", 70.0, true),
    category("Garbage", "Bills & Utilities", 35.0, true),
    category("Internet & Cable", "Bills & Utilities", 75.0, true),
    category("Phone", "Bills & Utilities", 135.0, true),
    category(SUBSCRIPTIONS, "Bills & Utilities", 75.0, false),
    category("Groceries", "Food & Dining", 1_100.0, false),
    category(RESTAURANTS, "Food & Dining", 450.0, false),
    category("Coffee Shops", "Food & Dining", 70.0, false),
    category(AUTO_PAYMENT, "Auto & Transport", 465.0, true),
    category("Gas", "Auto & Transport", 190.0, false),
    category("Parking & Tolls", "Auto & Transport", 30.0, false),
    category("Taxi & Ride Shares", "Auto & Transport", 45.0, false),
    category("Auto Maintenance", "Auto & Transport", 60.0, false),
    category("Child Care", "Children", 1_650.0, true),
    category("Child Activities", "Children", 80.0, false),
    category(SHOPPING, "Shopping", 400.0, false),
    category("Clothing", "Shopping", 130.0, false),
    category(TRAVEL, "Travel & Lifestyle", 600.0, false),
    category(ENTERTAINMENT, "Travel & Lifestyle", 120.0, false),
    category("Fitness", "Travel & Lifestyle", 54.0, true),
    category("Pets", "Travel & Lifestyle", 100.0, false),
    category("Medical", "Health & Wellness", 90.0, false),
    category("Charity", "Gifts & Donations", 50.0, true),
    category("Gifts", "Gifts & Donations", 50.0, false),
    category("Insurance", "Financial", 214.0, true),
    category(PLAIN_TRANSFER, "Transfers", 0.0, false),
    category(CARD_PAYMENT, "Transfers", 0.0, false),
];

/// income, expense or transfer, from the group a category belongs to
fn group_type(category: &str) -> &'static str {
    let group = CATEGORIES.iter().find(|c| c.name == category).map(|c| c.group);
    GROUPS.iter().find(|g| Some(g.0) == group).map(|g| g.1).unwrap_or(TRANSFER)
}

/// `Food & Dining` as `food-dining`, for the ids Monarch would have given
fn slug(name: &str) -> String {
    let words: Vec<String> = name
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect();
    words.join("-")
}

/// A loan paid monthly from checking: its account, who is paid, and on what day
struct Loan {
    account: usize,
    category: &'static str,
    day: u32,
    /// Taxes and insurance collected with the payment
    escrow: f64,
}

const LOANS: [Loan; 2] = [
    Loan { account: MORTGAGE, category: MORTGAGE_PAYMENT, day: 1, escrow: 560.0 },
    Loan { account: AUTO_LOAN, category: AUTO_PAYMENT, day: 12, escrow: 0.0 },
];

/// A bill that comes on the same day of the month
struct Bill {
    merchant: &'static str,
    category: &'static str,
    account: usize,
    day: u32,
    amount: f64,
    /// How far either side of `amount` it runs, as a share of it
    varies: f64,
    /// Months from one to the next, and which of them it falls in
    every: u32,
    phase: u32,
}

const fn monthly(merchant: &'static str, category: &'static str, account: usize, day: u32, amount: f64) -> Bill {
    Bill { merchant, category, account, day, amount, varies: 0.0, every: 1, phase: 0 }
}

impl Bill {
    const fn varying(mut self, share: f64) -> Self {
        self.varies = share;
        self
    }

    const fn every(mut self, months: u32, phase: u32) -> Self {
        self.every = months;
        self.phase = phase;
        self
    }
}

const QUARTERLY: u32 = 3;
const YEARLY: u32 = 12;

const BILLS: [Bill; 17] = [
    monthly("Little Sprouts Preschool", "Child Care", CHECKING, 1, 1_650.0),
    monthly("Cascade Power & Light", "Gas & Electric", CHECKING, 18, 148.0).varying(0.28),
    monthly("Cedar Valley Water", "Water", CHECKING, 9, 68.0).varying(0.15),
    monthly("Greenway Waste Services", "Garbage", CHECKING, 6, 96.4).every(QUARTERLY, 1),
    monthly("Lakeshore Mutual Insurance", "Insurance", CHECKING, 15, 214.0),
    monthly("Community Food Bank", "Charity", CHECKING, 20, 50.0),
    monthly("Beacon Fiber Internet", "Internet & Cable", TRAVEL_CARD, 22, 75.0),
    monthly("Summit Wireless", "Phone", TRAVEL_CARD, 5, 132.4),
    monthly("Northside Fitness", "Fitness", CASH_CARD, 3, 54.0),
    monthly("Streamhouse", SUBSCRIPTIONS, TRAVEL_CARD, 11, 17.99),
    monthly("Wavelength Music", SUBSCRIPTIONS, TRAVEL_CARD, 24, 11.99),
    monthly("Cloudnest Storage", SUBSCRIPTIONS, TRAVEL_CARD, 7, 2.99),
    monthly("The Daily Ledger", SUBSCRIPTIONS, TRAVEL_CARD, 14, 12.0),
    monthly("Pinecrest Photo Software", SUBSCRIPTIONS, TRAVEL_CARD, 19, 119.0).every(YEARLY, 4),
    monthly("Harbor Auto Club", "Auto Maintenance", CASH_CARD, 8, 68.0).every(YEARLY, 9),
    monthly("Parkside Wholesale Membership", SHOPPING, CASH_CARD, 27, 65.0).every(YEARLY, 1),
    monthly("City Science Museum Membership", ENTERTAINMENT, TRAVEL_CARD, 2, 145.0).every(YEARLY, 6),
];

/// A salary paid into checking every two weeks
struct Paycheck {
    employer: &'static str,
    amount: f64,
    /// A day one was paid, as (year, month, day)
    paid_on: (i32, u32, u32),
    /// What goes into the 401(k) with each one, the employer's match included
    retirement: f64,
}

const PAYCHECKS_PAID: [Paycheck; 2] = [
    Paycheck { employer: COMPANY, amount: 4_312.18, paid_on: (2024, 1, 5), retirement: 975.0 },
    Paycheck { employer: "Riverbend Schools", amount: 2_468.4, paid_on: (2024, 1, 11), retirement: 0.0 },
];

/// What a salary is multiplied by each new year
const RAISE: f64 = 1.035;
/// The year the paycheck amounts above were paid
const BASE_YEAR: i32 = 2024;

/// Money moved between the household's own accounts each month
struct Standing {
    name: &'static str,
    to: usize,
    day: u32,
    amount: f64,
}

const STANDING_TRANSFERS: [Standing; 3] = [
    Standing { name: "Granite Brokerage Deposit", to: BROKERAGE, day: 4, amount: 2_000.0 },
    Standing { name: "Granite Roth IRA Contribution", to: ROTH, day: 4, amount: 583.33 },
    Standing { name: "Lakeshore 529 Contribution", to: COLLEGE, day: 5, amount: 400.0 },
];

/// Once the month's big bills are paid, what checking holds over a cushion is
/// moved to savings, in round amounts and never less than a token one
const SWEEP_DAY: u32 = 2;
const SWEEP_NAME: &str = "Transfer to Savings";
const CHECKING_CUSHION: f64 = 8_000.0;
const SWEEP_STEP: f64 = 100.0;
const LEAST_SWEEP: f64 = 500.0;

/// A credit card: the day its statement closes and the day that statement is paid in full
struct Card {
    account: usize,
    closes: u32,
    paid: u32,
    payment: &'static str,
}

const CARDS: [Card; 2] = [
    Card { account: TRAVEL_CARD, closes: 22, paid: 16, payment: "Summit Trust Card Payment" },
    Card { account: CASH_CARD, closes: 9, paid: 3, payment: "Meridian Bank Card Payment" },
];

/// Somewhere money is spent now and then: how often, how much, and on which card
struct Habit {
    category: &'static str,
    /// The first is visited most
    merchants: &'static [&'static str],
    per_month: f64,
    low: f64,
    high: f64,
    account: usize,
}

const HABITS: [Habit; 15] = [
    Habit {
        category: "Groceries",
        merchants: &["Maple Street Market", "Green Basket Co-op", "Parkside Wholesale"],
        per_month: 11.0,
        low: 24.0,
        high: 215.0,
        account: CASH_CARD,
    },
    Habit {
        category: RESTAURANTS,
        merchants: &[
            "Juniper Table",
            "Copper Kettle Diner",
            "Noodle Harbor",
            "Taqueria El Faro",
            "Slate & Rye Tavern",
            "Golden Lantern Thai",
            "Pizzeria Fiamma",
        ],
        per_month: 9.0,
        low: 14.0,
        high: 124.0,
        account: TRAVEL_CARD,
    },
    Habit {
        category: "Coffee Shops",
        merchants: &["Blue Heron Coffee", "Driftwood Roasters"],
        per_month: 10.0,
        low: 4.25,
        high: 14.5,
        account: TRAVEL_CARD,
    },
    Habit {
        category: "Gas",
        merchants: &["Summit Fuel", "Lakeside Gas & Go"],
        per_month: 4.0,
        low: 34.0,
        high: 72.0,
        account: CASH_CARD,
    },
    Habit {
        category: SHOPPING,
        merchants: &["Corner General", "Birch & Linen", "Orchard Lane Books", "Marigold Home Goods", "Pebble Electronics"],
        per_month: 5.0,
        low: 9.0,
        high: 260.0,
        account: TRAVEL_CARD,
    },
    Habit {
        category: "Clothing",
        merchants: &["Willow Thread", "Fieldstone Outfitters"],
        per_month: 1.5,
        low: 28.0,
        high: 190.0,
        account: TRAVEL_CARD,
    },
    Habit {
        category: ENTERTAINMENT,
        merchants: &["Starlight Cinemas", "Timberline Climbing Gym", "Cedar Lanes Bowling"],
        per_month: 2.5,
        low: 12.0,
        high: 85.0,
        account: TRAVEL_CARD,
    },
    Habit {
        category: "Medical",
        merchants: &["Meadow Pharmacy", "Lakeview Pediatrics", "Riverbend Family Dental"],
        per_month: 1.2,
        low: 8.0,
        high: 180.0,
        account: CASH_CARD,
    },
    Habit {
        category: "Pets",
        merchants: &["Paws & Whiskers Supply", "Valley Veterinary Clinic"],
        per_month: 1.5,
        low: 22.0,
        high: 160.0,
        account: CASH_CARD,
    },
    Habit {
        category: HOME_IMPROVEMENT,
        merchants: &["Timber & Nail Hardware", "Greenleaf Garden Center"],
        per_month: 1.5,
        low: 12.0,
        high: 240.0,
        account: CASH_CARD,
    },
    Habit {
        category: "Taxi & Ride Shares",
        merchants: &["Hopalong Rides"],
        per_month: 2.0,
        low: 9.0,
        high: 42.0,
        account: TRAVEL_CARD,
    },
    Habit {
        category: "Parking & Tolls",
        merchants: &["CityPark Garage", "Bay Bridge Tolls"],
        per_month: 2.0,
        low: 4.0,
        high: 26.0,
        account: TRAVEL_CARD,
    },
    Habit {
        category: "Gifts",
        merchants: &["Paper Crane Gifts", "Bloom & Stem Florist"],
        per_month: 0.8,
        low: 18.0,
        high: 120.0,
        account: TRAVEL_CARD,
    },
    Habit {
        category: "Child Activities",
        merchants: &["Little Kickers Soccer", "Tumble Town"],
        per_month: 1.0,
        low: 25.0,
        high: 120.0,
        account: CASH_CARD,
    },
    Habit {
        category: "Auto Maintenance",
        merchants: &["Precision Auto Care"],
        per_month: 0.3,
        low: 60.0,
        high: 540.0,
        account: CASH_CARD,
    },
];

/// Days in a month, near enough for turning a monthly rate into a daily chance
const DAYS_PER_MONTH: f64 = 30.4;

/// A trip is taken every so many months: flights early in the month, then the
/// hotel and what was spent there
const MONTHS_BETWEEN_TRIPS: u32 = 4;
const TRIP_PHASE: u32 = 3;
const TRIP_TAG: &str = "Vacation";
/// What is bought for a trip, as (day of the month, merchant, category, least, most)
const TRIP_SPENDING: [(u32, &str, &str, f64, f64); 6] = [
    (6, "Skyward Airlines", TRAVEL, 310.0, 680.0),
    (6, "Skyward Airlines", TRAVEL, 310.0, 680.0),
    (17, "Tidepool Seafood House", RESTAURANTS, 70.0, 190.0),
    (18, "Island Kayak Tours", TRAVEL, 90.0, 260.0),
    (19, "Lighthouse Point Cafe", RESTAURANTS, 30.0, 95.0),
    (20, "Harbor Lights Hotel", TRAVEL, 620.0, 1_380.0),
];

/// Things that happen once a year and do not belong in everyday income and
/// spending: a tax refund, as (month, day, payer), and a large purchase, as
/// (month, day, merchant, category, amount). Which purchase goes by the year.
const TAX_REFUND: (u32, u32, &str) = (4, 14, "US Treasury Tax Refund");
/// What the refund came to, by the year
const TAX_REFUNDS: [f64; 3] = [2_184.0, 3_406.0, 1_672.0];
const PROJECTS: [(u32, u32, &str, &str, f64); 3] = [
    (5, 6, "Ridgeline Roofing", HOME_IMPROVEMENT, -6_850.0),
    (8, 19, "Evergreen Tree Service", HOME_IMPROVEMENT, -1_950.0),
    (2, 20, "Northgate Appliance", SHOPPING, -2_480.0),
];

const SAVINGS_WITHDRAWAL: &str = "Transfer from Savings";
/// Should checking end a day with less than the floor, savings tops it up to this
const CHECKING_FLOOR: f64 = 500.0;
const CHECKING_TOP_UP: f64 = 2_500.0;

/// A dividend is paid into the brokerage account on this day of every third month
const DIVIDEND_DAY: u32 = 20;
const DIVIDEND_YIELD: f64 = 0.0035;
const DIVIDEND_PAYER: &str = "Granite Total Market Index Fund";

/// The market's average daily gain and how far a day strays from it
const MARKET_DRIFT: f64 = 0.0004;
const MARKET_SWING: f64 = 0.0065;
/// How much of the market's move each investment account takes
const MARKET_SHARE: [(usize, f64); 4] = [(RETIREMENT, 0.85), (ROTH, 0.95), (BROKERAGE, 1.1), (COLLEGE, 0.7)];
/// The range the house's estimated value moves by each month
const HOME_APPRECIATION: (f64, f64) = (-0.002, 0.008);

struct Fund {
    ticker: &'static str,
    name: &'static str,
    asset_class: &'static str,
    /// Monarch's security type
    kind: &'static str,
    price: f64,
    /// What it is worth over what was paid for it
    gain: f64,
    /// How much of the market's day it moves by
    market_share: f64,
}

const TARGET_FUND: usize = 0;
const INDEX_FUND: usize = 1;
const INTERNATIONAL_FUND: usize = 2;
const TOTAL_MARKET_FUND: usize = 3;
const BOND_FUND: usize = 4;
const DIVIDEND_ETF: usize = 5;
const FIRST_STOCK: usize = 6;
const SECOND_STOCK: usize = 7;
const MONEY_FUND: usize = 8;
const COLLEGE_PORTFOLIO: usize = 9;

const FUNDS: [Fund; 10] = [
    Fund { ticker: "NFTRX", name: "Northfield Target Retirement 2055 Fund", asset_class: "mixed", kind: "Mutual Fund", price: 31.42, gain: 1.31, market_share: 0.8 },
    Fund { ticker: "NFSPX", name: "Northfield 500 Index Fund", asset_class: "stocks", kind: "Mutual Fund", price: 212.77, gain: 1.44, market_share: 1.0 },
    Fund { ticker: "NFIIX", name: "Northfield International Index Fund", asset_class: "stocks", kind: "Mutual Fund", price: 18.91, gain: 1.12, market_share: 0.9 },
    Fund { ticker: "GRTMX", name: "Granite Total Market Index Fund", asset_class: "stocks", kind: "Mutual Fund", price: 128.36, gain: 1.38, market_share: 1.0 },
    Fund { ticker: "GRTBX", name: "Granite Total Bond Index Fund", asset_class: "bonds", kind: "Mutual Fund", price: 10.14, gain: 0.97, market_share: 0.15 },
    Fund { ticker: "BDGE", name: "Beacon Dividend Growth ETF", asset_class: "stocks", kind: "ETF", price: 74.55, gain: 1.22, market_share: 0.8 },
    Fund { ticker: "CDFG", name: "Cedar Forge Technologies", asset_class: "stocks", kind: "Stock", price: 186.2, gain: 1.87, market_share: 1.6 },
    Fund { ticker: "HCRN", name: "Harbor Crane Industries", asset_class: "stocks", kind: "Stock", price: 52.08, gain: 0.91, market_share: 1.2 },
    Fund { ticker: "GRMXX", name: "Granite Treasury Money Market Fund", asset_class: "cash", kind: "Cash", price: 1.0, gain: 1.0, market_share: 0.0 },
    Fund { ticker: "", name: "Lakeshore College 2038 Portfolio", asset_class: "mixed", kind: "Other", price: 19.63, gain: 1.17, market_share: 0.6 },
];

/// What each investment account holds, as (fund, share of the account)
const ALLOCATIONS: [(usize, &[(usize, f64)]); 4] = [
    (RETIREMENT, &[(TARGET_FUND, 0.55), (INDEX_FUND, 0.3), (INTERNATIONAL_FUND, 0.15)]),
    (ROTH, &[(TOTAL_MARKET_FUND, 0.8), (BOND_FUND, 0.2)]),
    (
        BROKERAGE,
        &[
            (TOTAL_MARKET_FUND, 0.42),
            (DIVIDEND_ETF, 0.2),
            (FIRST_STOCK, 0.14),
            (SECOND_STOCK, 0.09),
            (BOND_FUND, 0.08),
            (MONEY_FUND, 0.07),
        ],
    ),
    (COLLEGE, &[(COLLEGE_PORTFOLIO, 1.0)]),
];

/// Card purchases this recent have not settled yet
const PENDING_DAYS: i64 = 1;
/// Purchases this recent may still be waiting to be looked over, and how many are
const REVIEW_DAYS: i64 = 12;
const REVIEW_CHANCE: f64 = 0.4;

/// The credit score is reported on this day each month, for this many months
const SCORE_DAY: u32 = 24;
const SCORE_MONTHS: u32 = 14;
const FIRST_SCORE: f64 = 744.0;
const SCORE_GAIN_PER_MONTH: f64 = 2.6;
const SCORE_WOBBLE: i64 = 4;

/// Savings goals, as (name, target, planned each month, account, share of that account's balance)
const GOALS: [(&str, f64, f64, usize, f64); 3] = [
    ("Emergency Fund", 45_000.0, 900.0, SAVINGS, 0.4),
    ("Kitchen Remodel", 60_000.0, 800.0, SAVINGS, 0.42),
    ("Trip to Japan", 12_000.0, 300.0, SAVINGS, 0.09),
];

fn cents(amount: f64) -> f64 {
    (amount * CENTS_PER_DOLLAR).round() / CENTS_PER_DOLLAR
}

/// Whether `date` is the `day` of its month, or its last day in a month too short to have one
fn falls_on(date: NaiveDate, day: u32) -> bool {
    date.day() == day.min(month_end(date).day())
}

/// Months since year zero, for things that come round every so many months
fn month_number(date: NaiveDate) -> u32 {
    date.year() as u32 * MONTHS_PER_YEAR as u32 + date.month0()
}

/// A draw that is small more often than large: as likely to fall between
/// $10 and $20 as between $100 and $200
fn skewed(dice: &mut Dice, low: f64, high: f64) -> f64 {
    cents(low * (high / low).powf(dice.unit()))
}

/// A draw from roughly a bell curve around zero, about one wide
fn bell(dice: &mut Dice) -> f64 {
    /// Uniform draws summed, and what scales their sum to a spread of one
    const DRAWS: usize = 4;
    const SPREAD: f64 = 1.732_050_8;
    let sum: f64 = (0..DRAWS).map(|_| dice.unit()).sum();
    (sum - DRAWS as f64 / 2.0) * SPREAD
}

/// The household as it is lived forward
struct Household {
    seed: u64,
    today: NaiveDate,
    /// Signed balance of each account in `ACCOUNTS`
    balances: Vec<f64>,
    /// Each account's balance at the end of every day so far
    history: Vec<Vec<(NaiveDate, f64)>>,
    /// Oldest first
    transactions: Vec<Txn>,
    /// What each card's last statement came to and is still to be paid
    statements: Vec<f64>,
    /// The market's move on the latest trading day
    market: f64,
}

impl Household {
    /// Record a transaction and move the account's balance by it
    fn post(&mut self, date: NaiveDate, account: usize, amount: f64, merchant: &str, category: &str) -> &mut Txn {
        let amount = cents(amount);
        self.balances[account] += amount;
        self.transactions.push(Txn {
            id: format!("demo-{:06}", self.transactions.len() + 1),
            date,
            amount,
            credited: 0.0,
            is_matched_credit: false,
            merchant: merchant.to_string(),
            category: category.to_string(),
            group_type: group_type(category).to_string(),
            account_id: ACCOUNTS[account].id.to_string(),
            notes: String::new(),
            original_name: merchant.to_uppercase(),
            tags: String::new(),
            pending: false,
            recurring: false,
            needs_review: false,
            hide_from_reports: false,
            hidden_by_account: false,
            one_time: false,
        });
        let last = self.transactions.len() - 1;
        &mut self.transactions[last]
    }

    /// Move money between two of the household's accounts, recorded on both
    /// sides. `standing` says it is one that is made every month.
    fn transfer(&mut self, date: NaiveDate, from: usize, to: usize, amount: f64, name: &str, standing: bool) {
        let category = if CARDS.iter().any(|card| card.account == to) { CARD_PAYMENT } else { PLAIN_TRANSFER };
        self.post(date, from, -amount, name, category).recurring = standing;
        self.post(date, to, amount, name, category).recurring = standing;
    }

    fn one_time(&mut self, date: NaiveDate, account: usize, amount: f64, merchant: &str, category: &str) {
        let txn = self.post(date, account, amount, merchant, category);
        txn.one_time = true;
        txn.tags = ONE_TIME_TAG.to_string();
    }

    /// Everything that happens on one day
    fn day(&mut self, date: NaiveDate) {
        let mut dice = Dice::new(hash(self.seed, &date.to_string()));
        let month = month_number(date);

        for loan in &LOANS {
            let spec = &ACCOUNTS[loan.account];
            if let (true, Some(rate), Some(payment)) = (falls_on(date, loan.day), spec.rate, spec.payment) {
                let interest = -self.balances[loan.account] * rate / PERCENT / MONTHS_PER_YEAR;
                self.post(date, CHECKING, -(payment + loan.escrow), spec.institution, loan.category).recurring = true;
                self.balances[loan.account] = (self.balances[loan.account] + payment - interest).min(0.0);
            }
        }
        for bill in &BILLS {
            let wobble = 1.0 + bill.varies * (dice.unit() * 2.0 - 1.0);
            if falls_on(date, bill.day) && month % bill.every == bill.phase {
                self.post(date, bill.account, -bill.amount * wobble, bill.merchant, bill.category).recurring = true;
            }
        }
        for paycheck in &PAYCHECKS_PAID {
            let (year, month, day) = paycheck.paid_on;
            let first = NaiveDate::from_ymd_opt(year, month, day).unwrap_or(date);
            if (date - first).num_days().rem_euclid(DAYS_PER_PAY_PERIOD) == 0 {
                let amount = paycheck.amount * RAISE.powi(date.year() - BASE_YEAR);
                self.post(date, CHECKING, amount, paycheck.employer, PAYCHECKS).recurring = true;
                self.balances[RETIREMENT] += paycheck.retirement;
            }
        }
        for standing in &STANDING_TRANSFERS {
            if falls_on(date, standing.day) {
                self.transfer(date, CHECKING, standing.to, standing.amount, standing.name, true);
            }
        }
        if falls_on(date, SWEEP_DAY) {
            let spare = ((self.balances[CHECKING] - CHECKING_CUSHION) / SWEEP_STEP).floor() * SWEEP_STEP;
            self.transfer(date, CHECKING, SAVINGS, spare.max(LEAST_SWEEP), SWEEP_NAME, true);
        }
        for (index, card) in CARDS.iter().enumerate() {
            if falls_on(date, card.paid) && self.statements[index] > 0.0 {
                let owed = std::mem::take(&mut self.statements[index]);
                self.transfer(date, CHECKING, card.account, owed, card.payment, true);
            }
        }

        for habit in &HABITS {
            let (went, which, spent) = (dice.unit(), dice.unit(), skewed(&mut dice, habit.low, habit.high));
            if went < habit.per_month / DAYS_PER_MONTH {
                let merchant = habit.merchants[(which * which * habit.merchants.len() as f64) as usize];
                self.post(date, habit.account, -spent, merchant, habit.category);
            }
        }
        if month % MONTHS_BETWEEN_TRIPS == TRIP_PHASE {
            for (day, merchant, category, low, high) in TRIP_SPENDING {
                let spent = skewed(&mut dice, low, high);
                if falls_on(date, day) {
                    self.post(date, TRAVEL_CARD, -spent, merchant, category).tags = TRIP_TAG.to_string();
                }
            }
        }

        let (refund_month, refund_day, payer) = TAX_REFUND;
        if date.month() == refund_month && date.day() == refund_day {
            self.one_time(date, CHECKING, TAX_REFUNDS[date.year() as usize % TAX_REFUNDS.len()], payer, OTHER_INCOME);
        }
        let (project_month, project_day, contractor, category, cost) = PROJECTS[date.year() as usize % PROJECTS.len()];
        if date.month() == project_month && date.day() == project_day {
            // Saved up for, so the money comes out of savings first
            self.transfer(date, SAVINGS, CHECKING, -cost, SAVINGS_WITHDRAWAL, false);
            self.one_time(date, CHECKING, cost, contractor, category);
        }

        if date == month_end(date) {
            let interest = self.balances[SAVINGS] * SAVINGS_RATE / PERCENT / MONTHS_PER_YEAR;
            self.post(date, SAVINGS, interest, ACCOUNTS[SAVINGS].institution, INTEREST).recurring = true;
        }
        if falls_on(date, DIVIDEND_DAY) && month % QUARTERLY == 0 {
            let dividend = self.balances[BROKERAGE] * DIVIDEND_YIELD;
            self.post(date, BROKERAGE, dividend, DIVIDEND_PAYER, DIVIDENDS).recurring = true;
        }

        let swing = bell(&mut dice);
        if !matches!(date.weekday(), Weekday::Sat | Weekday::Sun) {
            self.market = MARKET_DRIFT + MARKET_SWING * swing;
            for (account, share) in MARKET_SHARE {
                self.balances[account] *= 1.0 + self.market * share;
            }
        }
        let appreciation = dice.between(HOME_APPRECIATION);
        if falls_on(date, 1) {
            self.balances[HOME] *= 1.0 + appreciation;
        }

        if self.balances[CHECKING] < CHECKING_FLOOR {
            let short = ((CHECKING_TOP_UP - self.balances[CHECKING]) / SWEEP_STEP).ceil() * SWEEP_STEP;
            self.transfer(date, SAVINGS, CHECKING, short, SAVINGS_WITHDRAWAL, false);
        }
        // A statement closes on what is owed once the day's purchases are in
        for (index, card) in CARDS.iter().enumerate() {
            if falls_on(date, card.closes) {
                self.statements[index] = -self.balances[card.account];
            }
        }
        for (account, balance) in self.balances.iter().enumerate() {
            self.history[account].push((date, *balance));
        }
    }

    /// Live every day from the start of the history to `today`
    fn lived(seed: u64, today: NaiveDate) -> Household {
        let mut household = Household {
            seed,
            today,
            balances: ACCOUNTS.iter().map(|account| account.opening).collect(),
            history: vec![Vec::new(); ACCOUNTS.len()],
            transactions: Vec::new(),
            statements: vec![0.0; CARDS.len()],
            market: 0.0,
        };
        let mut date = month_of(today.checked_sub_months(Months::new(HISTORY_MONTHS)).unwrap_or(today));
        while date <= today {
            household.day(date);
            date += Duration::days(1);
        }
        household.mark_recent();
        household
    }

    /// The latest card purchases have not settled, and some have yet to be looked over
    fn mark_recent(&mut self) {
        let (seed, today) = (self.seed, self.today);
        for txn in &mut self.transactions {
            let days_ago = (today - txn.date).num_days();
            let on_a_card = CARDS.iter().any(|card| ACCOUNTS[card.account].id == txn.account_id);
            let drawn_for = format!("review:{}:{}:{}", txn.date, txn.merchant, txn.amount);
            let chosen = Dice::new(hash(seed, &drawn_for)).chance(REVIEW_CHANCE);
            txn.pending = on_a_card && txn.amount < 0.0 && days_ago <= PENDING_DAYS;
            txn.needs_review = !txn.recurring && days_ago <= REVIEW_DAYS && chosen;
        }
    }

    fn accounts(&self) -> Vec<Account> {
        let synced = Utc::now();
        ACCOUNTS
            .iter()
            .zip(&self.balances)
            .map(|(spec, balance)| Account {
                id: spec.id.to_string(),
                name: spec.name.to_string(),
                mask: (!spec.mask.is_empty()).then(|| spec.mask.to_string()),
                kind: spec.kind.to_string(),
                kind_display: spec.kind_display().to_string(),
                subtype: spec.subtype.to_string(),
                institution: spec.institution.to_string(),
                balance: balance.abs(),
                signed_balance: *balance,
                is_asset: spec.is_asset(),
                is_hidden: false,
                include_in_net_worth: true,
                needs_attention: false,
                last_synced: Some(synced),
                credit_limit: spec.credit_limit,
                interest_rate: spec.rate,
                minimum_payment: spec.payment,
            })
            .collect()
    }

    /// Each investment account's balance, divided among the funds it holds
    fn positions(&self) -> Vec<Position> {
        let mut positions = Vec::new();
        for (account, funds) in ALLOCATIONS {
            for (fund, share) in funds {
                let (fund, value) = (&FUNDS[*fund], self.balances[account] * share);
                positions.push(Position {
                    account_id: ACCOUNTS[account].id.to_string(),
                    account_name: ACCOUNTS[account].name.to_string(),
                    name: fund.name.to_string(),
                    ticker: fund.ticker.to_string(),
                    kind: fund.kind.to_string(),
                    quantity: value / fund.price,
                    price: Some(fund.price),
                    value,
                    cost_basis: Some(value / fund.gain),
                });
            }
        }
        positions
    }

    /// The positions added up by fund, largest first
    fn holdings(&self, positions: &[Position]) -> Vec<Holding> {
        let mut holdings: Vec<Holding> = FUNDS
            .iter()
            .map(|fund| {
                let held = || positions.iter().filter(|position| position.name == fund.name);
                Holding {
                    name: fund.name.to_string(),
                    ticker: fund.ticker.to_string(),
                    asset_class: fund.asset_class.to_string(),
                    quantity: held().map(|position| position.quantity).sum(),
                    price: Some(fund.price),
                    value: held().map(|position| position.value).sum(),
                    cost_basis: Some(held().filter_map(|position| position.cost_basis).sum()),
                    // A fund the market does not move is flat, not down by nothing
                    day_change_pct: Some(if fund.market_share == 0.0 { 0.0 } else { self.market * fund.market_share * PERCENT }),
                }
            })
            .collect();
        holdings.sort_by(|a, b| b.value.total_cmp(&a.value));
        holdings
    }

    /// This month's plan against what has been earned and spent, as Monarch stores it
    fn budget(&self) -> Value {
        let month = month_of(self.today);
        let actual = |category: &CategorySpec| -> f64 {
            let total: f64 = self
                .transactions
                .iter()
                .filter(|txn| txn.category == category.name && month_of(txn.date) == month && !txn.one_time)
                .map(|txn| txn.amount)
                .sum();
            cents(if group_type(category.name) == INCOME { total } else { -total })
        };
        let budgeted = || CATEGORIES.iter().filter(|category| group_type(category.name) != TRANSFER);
        let line = |planned: f64, actual: f64| {
            json!({ "plannedAmount": planned, "actualAmount": actual, "remainingAmount": cents(planned - actual) })
        };
        let total = |wanted: &dyn Fn(&CategorySpec) -> bool| {
            let (mut planned, mut spent) = (0.0, 0.0);
            for category in budgeted().filter(|category| wanted(category)) {
                planned += category.planned;
                spent += actual(category);
            }
            line(planned, cents(spent))
        };
        let expense = |category: &CategorySpec| group_type(category.name) == EXPENSE;
        let by_category: Vec<Value> = budgeted()
            .map(|category| {
                let spent = actual(category);
                json!({
                    "category": { "id": slug(category.name) },
                    "monthlyAmounts": [{
                        "plannedCashFlowAmount": category.planned,
                        "actualAmount": spent,
                        "remainingAmount": cents(category.planned - spent),
                    }],
                })
            })
            .collect();
        let groups: Vec<Value> = GROUPS
            .iter()
            .enumerate()
            .map(|(order, (group, kind))| {
                let categories: Vec<Value> = CATEGORIES
                    .iter()
                    .filter(|category| category.group == *group)
                    .enumerate()
                    .map(|(order, category)| {
                        json!({ "id": slug(category.name), "name": category.name, "order": order, "excludeFromBudget": false })
                    })
                    .collect();
                json!({ "id": slug(group), "name": group, "type": kind, "order": order, "categories": categories })
            })
            .collect();
        json!({
            "budgetData": {
                "monthlyAmountsByCategory": by_category,
                "totalsByMonth": [{
                    "month": month.format("%Y-%m-%d").to_string(),
                    "totalIncome": total(&|category| group_type(category.name) == INCOME),
                    "totalExpenses": total(&expense),
                    "totalFixedExpenses": total(&|category| expense(category) && category.fixed),
                    "totalFlexibleExpenses": total(&|category| expense(category) && !category.fixed),
                }],
            },
            "categoryGroups": groups,
        })
    }

    /// What is being saved up for, as Monarch stores it
    fn goals(&self) -> Value {
        let saved_this_month = self.transactions.iter().any(|txn| {
            txn.account_id == ACCOUNTS[SAVINGS].id && txn.category == PLAIN_TRANSFER && month_of(txn.date) == month_of(self.today)
        });
        let goals: Vec<Value> = GOALS
            .iter()
            .map(|(name, target, planned, account, share)| {
                let balance = cents((self.balances[*account] * share).min(*target));
                json!({
                    "name": name,
                    "currentBalance": balance,
                    "targetAmount": target,
                    "progress": balance / target,
                    "plannedMonthlyContribution": planned,
                    "currentMonthActualBudgetAmount": if saved_this_month { *planned } else { 0.0 },
                    "netContribution": balance,
                    "allocationAmountsByAccount": [{
                        "account": { "id": ACCOUNTS[*account].id, "displayName": ACCOUNTS[*account].name },
                        "totalAmount": balance,
                    }],
                    "archivedAt": null,
                })
            })
            .collect();
        json!({ "savingsGoals": goals })
    }

    /// A credit score that has been creeping up, oldest first
    fn credit_scores(&self) -> Vec<(NaiveDate, i64)> {
        (0..SCORE_MONTHS)
            .filter_map(|month| {
                let reported = self.today.checked_sub_months(Months::new(SCORE_MONTHS - 1 - month))?.with_day(SCORE_DAY)?;
                let wobble =
                    Dice::new(hash(self.seed, &format!("score:{}", reported))).whole((-SCORE_WOBBLE, SCORE_WOBBLE));
                let score = (FIRST_SCORE + month as f64 * SCORE_GAIN_PER_MONTH) as i64 + wobble;
                (reported <= self.today).then_some((reported, score))
            })
            .collect()
    }
}

/// The value of a series of steps on `date`: its latest point on or before it
fn value_on(steps: &[(NaiveDate, f64)], date: NaiveDate) -> f64 {
    let after = steps.partition_point(|step| step.0 <= date);
    after.checked_sub(1).map(|index| steps[index].1).unwrap_or(0.0)
}

/// The made-up household's finances as of `today`
pub fn data(today: NaiveDate) -> Data {
    household(SEED, today)
}

/// The finances of the household that `seed` moves in
fn household(seed: u64, today: NaiveDate) -> Data {
    let household = Household::lived(seed, today);
    let equity = equity::fabricate(seed, today, COMPANY, OPTION_GRANTS, EQUITY_WORTH);
    // The options are worth what the grants say, on every day of the history
    let mut history = household.history.clone();
    let options = equity.value_history(today);
    history[EQUITY] = history[EQUITY].iter().map(|(date, _)| (*date, value_on(&options, *date))).collect();

    let days = history[CHECKING].len();
    let net_worth = (0..days)
        .map(|day| {
            let balances = || history.iter().map(|series| series[day].1);
            let assets: f64 = balances().filter(|balance| *balance > 0.0).sum();
            let liabilities: f64 = -balances().filter(|balance| *balance < 0.0).sum::<f64>();
            NetWorthPoint { date: history[CHECKING][day].0, net_worth: assets - liabilities, assets, liabilities }
        })
        .collect();

    let positions = household.positions();
    let holdings = household.holdings(&positions);
    let total_value: f64 = holdings.iter().map(|holding| holding.value).sum();
    let day_change: f64 = holdings.iter().map(|h| h.value * h.day_change_pct.unwrap_or(0.0)).sum();
    let mut transactions = household.transactions.clone();
    transactions.reverse();

    let mut data = Data {
        synced_at: Some(Utc::now()),
        accounts: household.accounts(),
        transactions,
        net_worth,
        balances: ACCOUNTS.iter().zip(history).map(|(spec, series)| (spec.id.to_string(), series)).collect::<HashMap<_, _>>(),
        portfolio: Portfolio {
            total_value,
            one_day_change_pct: if total_value > 0.0 { day_change / total_value } else { 0.0 },
        },
        holdings,
        positions,
        categories: CATEGORIES
            .iter()
            .map(|category| Category { name: category.name.to_string(), group_id: slug(category.group) })
            .collect(),
        groups: GROUPS.iter().map(|(name, _)| CategoryGroup { id: slug(name), name: name.to_string() }).collect(),
        budget: Some(household.budget()),
        goals: Some(household.goals()),
        credit_scores: household.credit_scores(),
        equity,
        demo: true,
        ..Data::default()
    };
    data.derive();
    data
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::forecast::Cadence;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn today() -> NaiveDate {
        date(2026, 10, 12)
    }

    fn held(data: &Data, index: usize) -> &Account {
        data.account(ACCOUNTS[index].id).unwrap()
    }

    #[test]
    fn the_same_day_always_gives_the_same_household() {
        let (first, again) = (data(today()), data(today()));
        assert_eq!(first.transactions.len(), again.transactions.len());
        let amounts = |data: &Data| -> Vec<i64> { data.transactions.iter().map(|t| (t.amount * 100.0) as i64).collect() };
        assert_eq!(amounts(&first), amounts(&again));
        assert_eq!(held(&first, BROKERAGE).balance, held(&again, BROKERAGE).balance);
    }

    #[test]
    fn another_seed_moves_another_household_in_that_also_stays_solvent() {
        let usual = data(today());
        for seed in 1..=12 {
            let other = household(seed, today());
            assert_ne!(other.transactions.len(), usual.transactions.len());
            assert!(other.balances[ACCOUNTS[CHECKING].id].iter().all(|(_, balance)| *balance >= CHECKING_FLOOR));
            assert!(other.balances[ACCOUNTS[SAVINGS].id].iter().all(|(_, balance)| *balance > 0.0));
        }
    }

    #[test]
    fn a_later_day_adds_to_the_history_without_rewriting_it() {
        let (earlier, later) = (data(today()), data(today() + Duration::days(3)));
        let on = |data: &Data, day: NaiveDate| -> Vec<(String, i64)> {
            let mut rows: Vec<(String, i64)> = data
                .transactions
                .iter()
                .filter(|t| t.date == day)
                .map(|t| (t.merchant.clone(), (t.amount * 100.0) as i64))
                .collect();
            rows.sort();
            rows
        };
        for days_ago in [5, 40, 400] {
            let day = today() - Duration::days(days_ago);
            assert!(!on(&earlier, day).is_empty());
            assert_eq!(on(&earlier, day), on(&later, day));
        }
    }

    #[test]
    fn nothing_is_dated_after_today_and_the_newest_comes_first() {
        let data = data(today());
        assert!(data.transactions.iter().all(|t| t.date <= today()));
        assert!(data.transactions.windows(2).all(|pair| pair[0].date >= pair[1].date));
        assert!(data.transactions.len() > 1500, "{}", data.transactions.len());
        assert_eq!(data.net_worth.last().unwrap().date, today());
        assert!(data.demo && !data.anonymized);
    }

    #[test]
    fn balances_follow_from_the_transactions() {
        let data = data(today());
        for index in [CHECKING, SAVINGS, TRAVEL_CARD, CASH_CARD] {
            let moved: f64 = data.transactions.iter().filter(|t| t.account_id == ACCOUNTS[index].id).map(|t| t.amount).sum();
            let balance = held(&data, index).signed_balance;
            assert!((ACCOUNTS[index].opening + moved - balance).abs() < 0.01, "{}", ACCOUNTS[index].name);
        }
        // The household lives within its means
        assert!(data.balances[ACCOUNTS[CHECKING].id].iter().all(|(_, balance)| *balance >= CHECKING_FLOOR));
        assert!(held(&data, SAVINGS).signed_balance > ACCOUNTS[SAVINGS].opening);
        // Cards are paid off every month, so never run far up
        for card in &CARDS {
            let limit = ACCOUNTS[card.account].credit_limit.unwrap();
            assert!(data.balances[ACCOUNTS[card.account].id].iter().all(|(_, owed)| -owed < limit));
        }
        // Loans are being paid down
        for loan in &LOANS {
            let owed = held(&data, loan.account).balance;
            assert!(owed > 0.0 && owed < -ACCOUNTS[loan.account].opening);
        }
    }

    #[test]
    fn net_worth_is_what_the_accounts_add_up_to() {
        let data = data(today());
        let latest = data.latest_net_worth().unwrap();
        let total: f64 = data.accounts.iter().map(|a| a.signed_balance).sum();
        assert!((latest.net_worth - total).abs() < 0.01, "{} {}", latest.net_worth, total);
        assert!((latest.assets - latest.liabilities - latest.net_worth).abs() < 0.01);
        // The options count for what the grants are worth
        let options = held(&data, EQUITY);
        assert_eq!(options.balance, data.equity.summary(1.0).vested_value());
        assert!((options.balance / EQUITY_WORTH - 1.0).abs() < 0.15, "{}", options.balance);
    }

    #[test]
    fn holdings_add_up_to_the_investment_accounts() {
        let data = data(today());
        let invested: f64 = data.accounts.iter().filter(|a| a.kind == "brokerage").map(|a| a.balance).sum();
        let held: f64 = data.holdings.iter().map(|h| h.value).sum();
        let positioned: f64 = data.positions.iter().map(|p| p.value).sum();
        assert!((invested - held).abs() < 0.01 && (invested - positioned).abs() < 0.01);
        assert!((data.portfolio.total_value - invested).abs() < 0.01);
        assert!(data.holdings.iter().all(|h| h.value > 0.0));
        for (_, funds) in ALLOCATIONS {
            assert!((funds.iter().map(|fund| fund.1).sum::<f64>() - 1.0).abs() < 1e-9);
        }
    }

    #[test]
    fn the_bills_and_paychecks_are_found_as_recurring() {
        let data = data(today());
        let stream = |merchant: &str, account: usize| {
            data.streams.iter().find(|s| s.merchant == merchant && s.account_id == ACCOUNTS[account].id)
        };
        for paycheck in &PAYCHECKS_PAID {
            let found = stream(paycheck.employer, CHECKING).unwrap();
            assert_eq!(found.cadence, Cadence::Biweekly);
            assert!(found.is_income());
        }
        assert_eq!(stream(ACCOUNTS[MORTGAGE].institution, CHECKING).unwrap().cadence, Cadence::Monthly);
        assert_eq!(stream("Streamhouse", TRAVEL_CARD).unwrap().cadence, Cadence::Monthly);
        assert_eq!(stream("Pinecrest Photo Software", TRAVEL_CARD).unwrap().cadence, Cadence::Yearly);
        assert!(stream("Cascade Power & Light", CHECKING).unwrap().variable);
        // Shopping around is not a bill
        assert!(data.streams.iter().all(|s| s.merchant != "Maple Street Market" && s.merchant != "Blue Heron Coffee"));
    }

    #[test]
    fn every_transaction_is_in_a_category_the_budget_knows() {
        let data = data(today());
        for txn in &data.transactions {
            assert!(data.categories.iter().any(|c| c.name == txn.category), "{}", txn.category);
            assert!([INCOME, EXPENSE, TRANSFER].contains(&txn.group_type.as_str()));
        }
        for category in &data.categories {
            assert!(data.groups.iter().any(|g| g.id == category.group_id), "{}", category.name);
        }
        let budget = data.budget.as_ref().unwrap();
        let totals = &budget["budgetData"]["totalsByMonth"][0];
        assert_eq!(totals["month"], "2026-10-01");
        let (fixed, flexible) = (&totals["totalFixedExpenses"], &totals["totalFlexibleExpenses"]);
        for field in ["plannedAmount", "actualAmount"] {
            let whole = totals["totalExpenses"][field].as_f64().unwrap();
            assert!((fixed[field].as_f64().unwrap() + flexible[field].as_f64().unwrap() - whole).abs() < 0.01);
            assert!(whole > 0.0);
        }
        assert_eq!(data.goals.as_ref().unwrap()["savingsGoals"].as_array().unwrap().len(), GOALS.len());
        assert!(data.credit_scores.len() >= SCORE_MONTHS as usize - 1);
    }

    #[test]
    fn some_months_have_a_one_time_item_and_every_month_has_income() {
        let data = data(today());
        assert!(data.months.len() as u32 >= HISTORY_MONTHS);
        assert!(data.months.iter().all(|m| m.income > 0.0 && m.expense > 0.0));
        assert!(data.months.iter().any(|m| m.one_time_income > 0.0));
        assert!(data.months.iter().any(|m| m.one_time_expense > 0.0));
    }
}
