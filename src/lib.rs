//! Monarch Money Scraper Library
//!
//! This library provides functionality to scrape financial data from Monarch Money's
//! GraphQL API and store it in PostgreSQL for analysis.

pub mod client;
pub mod config;
pub mod credits;
pub mod db;
pub mod dedupe;
pub mod display;
pub mod equity;
pub mod logging;
pub mod models;
pub mod queries;

pub use client::MonarchClient;
pub use config::Config;
pub use db::Database;

pub mod api;
