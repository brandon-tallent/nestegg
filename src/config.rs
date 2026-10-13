//! CLI configuration and argument parsing

use clap::Parser;
use std::fs;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "monarch-scraper")]
#[command(about = "Scrape financial data from Monarch Money")]
#[command(version)]
pub struct Config {
    /// Path to auth token file (contains "Token <token>"; legacy, Monarch's web app now uses cookies)
    #[arg(short = 't', long, env = "MONARCH_AUTH_TOKEN_PATH")]
    pub auth_token_file: Option<PathBuf>,

    /// Auth token directly (overrides file)
    #[arg(long, env = "MONARCH_AUTH_TOKEN")]
    pub auth_token: Option<String>,

    /// Path to cookies file or Playwright state JSON file (session cookie auth)
    #[arg(short, long, env = "MONARCH_COOKIES_PATH")]
    pub cookies: Option<PathBuf>,

    /// Path to Playwright browser state file (session cookie auth)
    #[arg(long, env = "MONARCH_STATE_PATH")]
    pub state_file: Option<PathBuf>,

    /// Path to device-uuid file (or set MONARCH_DEVICE_UUID env var)
    #[arg(short = 'u', long, env = "MONARCH_DEVICE_UUID_PATH")]
    pub device_uuid_file: Option<PathBuf>,

    /// Device UUID (overrides file)
    #[arg(long, env = "MONARCH_DEVICE_UUID")]
    pub device_uuid: Option<String>,

    /// PostgreSQL database URL
    #[arg(short, long, env = "DATABASE_URL")]
    pub database: Option<String>,

    /// Output as JSON instead of table format
    #[arg(long)]
    pub json: bool,

    /// Verbose output
    #[arg(short, long)]
    pub verbose: bool,

    /// Number of days of transactions to fetch; 0 fetches the whole history
    #[arg(long, default_value = "0")]
    pub transaction_days: u32,

    /// Number of days of net worth history to fetch; 0 fetches all of it
    #[arg(long, default_value = "0")]
    pub history_days: u32,

    /// Skip fetching transactions (faster for balance-only runs)
    #[arg(long)]
    pub skip_transactions: bool,

    /// Skip fetching holdings
    #[arg(long)]
    pub skip_holdings: bool,

    /// Skip fetching recurring streams
    #[arg(long)]
    pub skip_recurring: bool,

    /// Skip fetching net worth history and per-account balance history
    #[arg(long)]
    pub skip_history: bool,

    /// Skip fetching budgets, goals, credit score and household documents
    #[arg(long)]
    pub skip_documents: bool,

    /// Ask Monarch to re-sync all accounts with their institutions and wait
    /// for that to finish before scraping
    #[arg(long, env = "MONARCH_FORCE_REFRESH")]
    pub force_refresh: bool,

    /// Maximum seconds to wait for a force refresh before scraping anyway
    #[arg(long, env = "MONARCH_FORCE_REFRESH_TIMEOUT", default_value = "600")]
    pub force_refresh_timeout: u64,

    /// Refile card statement credits (membership refunds, monthly allowances)
    /// under the merchant and category of the purchase each one refunds, so
    /// the two net out. Changes Monarch; no scrape is run.
    #[arg(long, env = "MONARCH_RECONCILE_CREDITS")]
    pub reconcile_credits: bool,

    /// Delete re-imported duplicate transactions from the named account,
    /// keeping the earliest copy of each. Some connections (Apple Card) import
    /// the whole history again each time they are set up. Deletes from
    /// Monarch and cannot be undone; no scrape is run.
    #[arg(long, value_name = "ACCOUNT")]
    pub dedupe_account: Option<String>,

    /// Load equity holdings from a Carta capture (the bundle file written by
    /// `npm run carta` in browser/) into the database. Monarch is not
    /// contacted and no scrape is run.
    #[arg(long, value_name = "BUNDLE")]
    pub load_carta: Option<PathBuf>,

    /// With --dedupe-account, delete at most this many (to test on a few first)
    #[arg(long, value_name = "N")]
    pub limit: Option<usize>,

    /// With --reconcile-credits or --dedupe-account, list the changes without
    /// making them
    #[arg(long)]
    pub dry_run: bool,

    /// List Monarch merchants matching NAME with their ids and transaction
    /// counts, including ones no transaction uses. Repeatable; no scrape is run.
    #[arg(long, value_name = "NAME")]
    pub find_merchant: Vec<String>,

    /// Merge merchant FROM into merchant INTO (Monarch merchant ids, as
    /// FROM=INTO), moving its transactions and deleting FROM. Repeatable.
    /// Changes Monarch itself and cannot be undone; no scrape is run.
    #[arg(long, value_name = "FROM=INTO")]
    pub merge_merchant: Vec<String>,

    /// Rename a merchant in Monarch, as ID=NAME. Repeatable; no scrape is run.
    #[arg(long, value_name = "ID=NAME")]
    pub rename_merchant: Vec<String>,

    /// Add a tag to one transaction, as TRANSACTION_ID=TAG NAME, creating the
    /// tag if needed. Repeatable; no scrape is run.
    #[arg(long, value_name = "ID=TAG")]
    pub tag_transaction: Vec<String>,

    /// Remove a tag from one transaction, as TRANSACTION_ID=TAG NAME.
    /// Repeatable; no scrape is run.
    #[arg(long, value_name = "ID=TAG")]
    pub untag_transaction: Vec<String>,

    /// Split one transaction into separately categorized parts, as
    /// TRANSACTION_ID=AMOUNT:CATEGORY;AMOUNT:CATEGORY. The amounts must add up
    /// to the transaction's amount. Repeatable; no scrape is run.
    #[arg(long, value_name = "ID=PARTS")]
    pub split_transaction: Vec<String>,

    /// Recategorize one transaction, as TRANSACTION_ID=CATEGORY NAME.
    /// Repeatable; no scrape is run.
    #[arg(long, value_name = "ID=CATEGORY")]
    pub set_transaction_category: Vec<String>,

    /// Move one transaction to another merchant, as TRANSACTION_ID=NAME.
    /// Repeatable; no scrape is run.
    #[arg(long, value_name = "ID=NAME")]
    pub set_transaction_merchant: Vec<String>,

    /// Delete the rule(s) that rename merchant MATCH. Repeatable; no scrape is run.
    #[arg(long, value_name = "MATCH")]
    pub delete_merchant_rule: Vec<String>,

    /// Create a Monarch rule so future transactions from merchant MATCH are
    /// filed under merchant TARGET, as MATCH=TARGET (names). Repeatable.
    /// Skipped when a rule already renames MATCH; no scrape is run.
    #[arg(long, value_name = "MATCH=TARGET")]
    pub merchant_rule: Vec<String>,

    /// Limit the rule that sets merchant MATCH's category to one account, as
    /// MATCH=ACCOUNT NAME, so it stops catching the same merchant name
    /// elsewhere. Repeatable; no scrape is run.
    #[arg(long, value_name = "MATCH=ACCOUNT")]
    pub limit_category_rule: Vec<String>,

    /// Create a Monarch rule so future transactions whose bank statement text
    /// is exactly STATEMENT are filed under CATEGORY, as STATEMENT=CATEGORY
    /// NAME. Repeatable. Skipped when a rule already matches STATEMENT; no
    /// scrape is run.
    #[arg(long, value_name = "STATEMENT=CATEGORY")]
    pub statement_category_rule: Vec<String>,

    /// With --statement-category-rule, apply the rules to this account only
    #[arg(long, value_name = "ACCOUNT")]
    pub rule_account: Option<String>,

    /// Ntfy notification URL for success/failure alerts
    #[arg(long, env = "NTFY_URL")]
    pub ntfy_url: Option<String>,
}

impl Config {
    /// Earliest date to fetch for a "last N days" option, where 0 means no limit
    pub fn since(days: u32) -> Option<chrono::NaiveDate> {
        (days > 0).then(|| (chrono::Utc::now() - chrono::Duration::days(days as i64)).date_naive())
    }

    /// Whether a legacy auth token was supplied (takes precedence over cookies)
    pub fn has_auth_token(&self) -> bool {
        self.auth_token.is_some() || self.auth_token_file.is_some()
    }

    /// Load and parse cookies from the cookies file or Playwright state file
    pub fn load_cookies(&self) -> anyhow::Result<Vec<(String, String)>> {
        // Try state file first if specified
        if let Some(ref state_path) = self.state_file {
            return self.load_cookies_from_state(state_path);
        }

        // Require cookies file if not using token auth
        let cookies_path = self.cookies.as_ref()
            .ok_or_else(|| anyhow::anyhow!("No credentials specified. Use --cookies or --state-file (or run 'npm run refresh' in the browser directory first)."))?;

        // Check if cookies file is a Playwright state JSON
        let content = fs::read_to_string(cookies_path)?;
        if content.trim_start().starts_with('{') {
            // It's a JSON file, try to parse as Playwright state
            return self.parse_playwright_state(&content);
        }

        // Plain text cookies file
        let cookies: Vec<(String, String)> = content
            .lines()
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .filter_map(|line| {
                let parts: Vec<&str> = line.splitn(2, '=').collect();
                if parts.len() == 2 {
                    Some((parts[0].to_string(), parts[1].to_string()))
                } else {
                    None
                }
            })
            .collect();

        if cookies.is_empty() {
            anyhow::bail!("No valid cookies found in {:?}", cookies_path);
        }

        Ok(cookies)
    }

    /// Load cookies from Playwright state JSON file
    fn load_cookies_from_state(&self, path: &PathBuf) -> anyhow::Result<Vec<(String, String)>> {
        let content = fs::read_to_string(path)?;
        self.parse_playwright_state(&content)
    }

    /// Parse Playwright browser state JSON and extract cookies
    fn parse_playwright_state(&self, content: &str) -> anyhow::Result<Vec<(String, String)>> {
        let state: serde_json::Value = serde_json::from_str(content)?;

        let cookies: Vec<(String, String)> = state["cookies"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("No cookies array in state file"))?
            .iter()
            .filter_map(|cookie| {
                let domain = cookie["domain"].as_str()?;
                // Only include monarch.com cookies
                if domain.contains("monarch") {
                    let name = cookie["name"].as_str()?;
                    let value = cookie["value"].as_str()?;
                    Some((name.to_string(), value.to_string()))
                } else {
                    None
                }
            })
            .collect();

        if cookies.is_empty() {
            anyhow::bail!("No Monarch cookies found in state file");
        }

        if self.verbose {
            eprintln!("Loaded {} cookies from Playwright state", cookies.len());
        }

        Ok(cookies)
    }

    /// Get the cookie header string for HTTP requests
    pub fn cookie_header(&self) -> anyhow::Result<String> {
        let cookies = self.load_cookies()?;
        Ok(cookies
            .iter()
            .map(|(name, value)| format!("{}={}", name, value))
            .collect::<Vec<_>>()
            .join("; "))
    }

    /// Get device UUID from file or environment
    pub fn get_device_uuid(&self) -> anyhow::Result<String> {
        // First check direct UUID argument
        if let Some(ref uuid) = self.device_uuid {
            return Ok(uuid.clone());
        }

        // Then check UUID file
        if let Some(ref path) = self.device_uuid_file {
            let content = fs::read_to_string(path)?;
            return Ok(content.trim().to_string());
        }

        // Finally, generate a new UUID (Monarch accepts this)
        let uuid = uuid::Uuid::new_v4().to_string();
        if self.verbose {
            eprintln!("Generated new device UUID: {}", uuid);
        }
        Ok(uuid)
    }

    /// Get auth token from file or environment
    /// Returns the full "Token <token>" string for the Authorization header
    pub fn get_auth_token(&self) -> anyhow::Result<String> {
        // First check direct token argument
        if let Some(ref token) = self.auth_token {
            let token = token.trim().to_string();
            if self.verbose {
                eprintln!("Using auth token from argument: {}...", &token[..20.min(token.len())]);
            }
            return Ok(token);
        }

        // Then check token file
        if let Some(ref path) = self.auth_token_file {
            let content = fs::read_to_string(path)?;
            let token = content.trim().to_string();
            if token.is_empty() {
                anyhow::bail!("Auth token file is empty: {:?}", path);
            }
            if self.verbose {
                eprintln!("Loaded auth token from file: {}...", &token[..20.min(token.len())]);
            }
            return Ok(token);
        }

        anyhow::bail!("No auth token provided. Use --auth-token-file or --auth-token, or run 'npm run refresh' in the browser directory first.")
    }
}
