//! Monarch Money Scraper CLI
//!
//! Fetches financial data from Monarch Money and optionally stores it in PostgreSQL.

use anyhow::{Context, Result};
use clap::Parser;
use colored::Colorize;
use monarch_scraper::client::DOCUMENTS;
use monarch_scraper::{Config, Database, MonarchClient};
use rust_decimal::Decimal;
use std::time::{Duration, Instant};
use tracing::{error, info, warn};

/// Color given to a tag this tool creates (Monarch's palette grey)
const NEW_TAG_COLOR: &str = "#7F8FA4";

/// How often to poll Monarch for force refresh progress
const FORCE_REFRESH_POLL_INTERVAL: Duration = Duration::from_secs(5);

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::parse();

    // Initialize logging (JSON format if database is specified, assuming container environment)
    monarch_scraper::logging::init(config.database.is_some() && !config.verbose);

    if config.verbose {
        eprintln!("{}", "Monarch Money Scraper".bold());
        eprintln!("Cookies file: {:?}", config.cookies);
        if let Some(ref db) = config.database {
            eprintln!("Database: {}", mask_connection_string(db));
        }
    }

    // Carta holdings come from a file, so no Monarch session is needed
    if let Some(ref bundle) = config.load_carta {
        return load_carta(&config, bundle).await;
    }

    // Create the Monarch client
    let client = MonarchClient::new(&config)?;

    if config.reconcile_credits {
        return reconcile_credits(&config, &client).await;
    }
    if let Some(ref account) = config.dedupe_account {
        return dedupe_account(&config, &client, account).await;
    }

    // Merchant edits are a separate, deliberate action: do them and stop
    if !config.find_merchant.is_empty()
        || !config.merge_merchant.is_empty()
        || !config.rename_merchant.is_empty()
        || !config.merchant_rule.is_empty()
        || !config.set_transaction_merchant.is_empty()
        || !config.set_transaction_category.is_empty()
        || !config.tag_transaction.is_empty()
        || !config.untag_transaction.is_empty()
        || !config.split_transaction.is_empty()
        || !config.delete_merchant_rule.is_empty()
        || !config.limit_category_rule.is_empty()
    {
        return edit_merchants(&config, &client).await;
    }

    // Connect to database if specified
    let db = if let Some(ref database_url) = config.database {
        Some(Database::connect(database_url).await?)
    } else {
        None
    };

    // Create run record if using database
    let run_id = if let Some(ref db) = db {
        let id = db.create_run(config.force_refresh).await?;
        info!(run_id = %id, "Created run record");
        Some(id)
    } else {
        None
    };

    // Execute the scrape, handling errors appropriately
    match execute_scrape(&config, &client, db.as_ref(), run_id).await {
        Ok(summary) => {
            // Update run status to success
            if let (Some(ref db), Some(run_id)) = (&db, run_id) {
                db.complete_run(
                    run_id,
                    summary.accounts_count as i32,
                    summary.transactions_count as i32,
                    summary.holdings_count as i32,
                    summary.net_worth,
                )
                .await?;
                info!(run_id = %run_id, "Run completed successfully");
            }

            // Send success notification
            if let Some(ref ntfy_url) = config.ntfy_url {
                send_notification(ntfy_url, true, &summary).await;
            }

            if !config.json && config.verbose {
                eprintln!("{}", "\n✓ Scrape completed successfully".green());
            }

            Ok(())
        }
        Err(e) => {
            // Update run status to failed
            if let (Some(ref db), Some(run_id)) = (&db, run_id) {
                let error_msg = format!("{:#}", e);
                db.fail_run(run_id, &error_msg).await.ok();
                error!(run_id = %run_id, error = %error_msg, "Run failed");
            }

            // Send failure notification
            if let Some(ref ntfy_url) = config.ntfy_url {
                send_failure_notification(ntfy_url, &e).await;
            }

            Err(e)
        }
    }
}

/// Trigger a refresh of every account at its institution and wait until
/// Monarch reports it finished. A refresh that outlasts the timeout is not
/// fatal: the scrape proceeds with whatever Monarch has by then.
async fn force_refresh_accounts(config: &Config, client: &MonarchClient) -> Result<()> {
    info!("Requesting force refresh of all accounts...");
    // Monarch rate-limits refreshes ("recently performed"); in that case its
    // data is already fresh, so carry on with the scrape.
    let operation_id = match client.force_refresh_all_accounts().await {
        Ok(id) => id,
        Err(e) => {
            warn!(error = %format!("{:#}", e), "Force refresh not started, scraping current data");
            return Ok(());
        }
    };
    info!(operation_id = ?operation_id, "Force refresh started");

    let timeout = Duration::from_secs(config.force_refresh_timeout);
    let started = Instant::now();

    loop {
        tokio::time::sleep(FORCE_REFRESH_POLL_INTERVAL).await;

        // The operation's own progress is authoritative. hasAccountsSyncing can
        // stay true indefinitely (e.g. a disconnected institution), so it is
        // only consulted when Monarch did not return an operation to track.
        let (operation, finished) = match operation_id {
            Some(ref id) => {
                let operation = client.fetch_force_refresh_operation(id).await?;
                let finished = operation.as_ref().is_some_and(|op| op.is_finished());
                if let Some(ref op) = operation {
                    info!(
                        state = ?op.state,
                        completed = ?op.completed_account_count,
                        total = ?op.total_account_count,
                        "Force refresh progress"
                    );
                }
                (operation, finished)
            }
            None => (None, !client.has_accounts_syncing().await?),
        };

        if finished {
            if let Some(op) = operation {
                for account in op.accounts.iter().filter(|a| {
                    a.timed_out == Some(true) || a.error_message.is_some()
                }) {
                    warn!(
                        account_id = ?account.account_id,
                        state = ?account.state,
                        timed_out = ?account.timed_out,
                        error = ?account.error_message,
                        "Account did not refresh cleanly"
                    );
                }
                let new_transactions: i64 = op
                    .accounts
                    .iter()
                    .filter_map(|a| a.new_transaction_count)
                    .sum();
                info!(
                    elapsed_secs = started.elapsed().as_secs(),
                    new_transactions,
                    "Force refresh complete"
                );
            } else {
                info!(elapsed_secs = started.elapsed().as_secs(), "Force refresh complete");
            }
            return Ok(());
        }

        if started.elapsed() >= timeout {
            warn!(
                timeout_secs = timeout.as_secs(),
                "Force refresh still running after timeout, scraping current data"
            );
            return Ok(());
        }
    }
}

/// Delete re-imported duplicate transactions from one account
async fn dedupe_account(config: &Config, client: &MonarchClient, account: &str) -> Result<()> {
    info!(account, "Fetching transactions to find re-imported duplicates...");
    let fetch = client.fetch_all_transactions(None).await?;
    let mut duplicates = monarch_scraper::dedupe::plan(&fetch.transactions.raw, account);
    let found = duplicates.len();
    let spending: f64 = duplicates.iter().filter(|d| d.amount < 0.0).map(|d| -d.amount).sum();
    info!(account, duplicates = found, spending = %format!("{:.2}", spending), dry_run = config.dry_run, "Re-imported duplicates");
    if let Some(limit) = config.limit {
        duplicates.truncate(limit);
    }

    for duplicate in &duplicates {
        if config.dry_run {
            println!(
                "{}  {:>10.2}  {}  (copy created {})",
                duplicate.date, duplicate.amount, duplicate.statement, duplicate.created
            );
        } else {
            client.delete_transaction(&duplicate.transaction_id).await?;
        }
    }
    if !config.dry_run {
        info!(deleted = duplicates.len(), remaining = found - duplicates.len(), "Deleted duplicates");
    }
    Ok(())
}

/// Replace the stored equity holdings with those in a Carta capture
async fn load_carta(config: &Config, bundle: &std::path::Path) -> Result<()> {
    let database_url = config.database.as_ref().context("--load-carta needs --database")?;
    let text = std::fs::read_to_string(bundle).with_context(|| format!("Could not read {}", bundle.display()))?;
    let holdings = monarch_scraper::equity::parse(&serde_json::from_str(&text)?)?;
    info!(
        issuers = holdings.issuers.len(),
        grants = holdings.grants.len(),
        vesting_events = holdings.vesting_events.len(),
        certificates = holdings.certificates.len(),
        exercises = holdings.exercises.len(),
        tenders = holdings.tenders.len(),
        "Carta holdings read"
    );
    if holdings.issuers.is_empty() {
        anyhow::bail!("No companies in {}; keeping the holdings already stored", bundle.display());
    }
    let db = Database::connect(database_url).await?;
    db.replace_equity(&holdings).await?;
    info!("Equity holdings stored");
    Ok(())
}

/// Refile statement credits so each nets out against the purchase it refunds
async fn reconcile_credits(config: &Config, client: &MonarchClient) -> Result<()> {
    info!("Fetching transactions to reconcile statement credits...");
    let fetch = client.fetch_all_transactions(None).await?;
    let fixes = monarch_scraper::credits::plan(&fetch.transactions.raw);
    info!(credits = fixes.len(), dry_run = config.dry_run, "Statement credits to refile");

    for fix in &fixes {
        println!(
            "{}  {:>9.2}  {}\n    merchant: {} -> {}\n    category: {} -> {}\n    refunds:  {}",
            fix.date,
            fix.amount,
            fix.statement,
            fix.from_merchant,
            fix.merchant.as_deref().unwrap_or("(unchanged)"),
            fix.from_category,
            fix.category.as_ref().map(|c| c.1.as_str()).unwrap_or("(unchanged)"),
            fix.paired_with.as_deref().unwrap_or("(no purchase found)"),
        );
        if !config.dry_run {
            client
                .update_transaction(
                    &fix.transaction_id,
                    fix.merchant.as_deref(),
                    fix.category.as_ref().map(|c| c.0.as_str()),
                )
                .await?;
        }
    }
    if !config.dry_run && !fixes.is_empty() {
        info!(updated = fixes.len(), "Refiled statement credits");
    }
    Ok(())
}

/// Split an `A=B` argument into its two halves
fn split_pair<'a>(argument: &'a str, flag: &str, shape: &str) -> Result<(&'a str, &'a str)> {
    argument
        .split_once('=')
        .map(|(left, right)| (left.trim(), right.trim()))
        .filter(|(left, right)| !left.is_empty() && !right.is_empty())
        .ok_or_else(|| anyhow::anyhow!("{} expects {}, got '{}'", flag, shape, argument))
}

/// Apply the requested merchant renames, merges and rules in Monarch.
/// Renames go first so a merge target can be renamed in the same run, and
/// rules last so they can name a merchant that a merge just kept.
async fn edit_merchants(config: &Config, client: &MonarchClient) -> Result<()> {
    for name in &config.find_merchant {
        for (id, found, transactions) in client.search_merchants(name).await? {
            println!("{}\t{}\t{}", id, transactions, found);
        }
    }
    for argument in &config.rename_merchant {
        let (merchant_id, name) = split_pair(argument, "--rename-merchant", "ID=NAME")?;
        let now = client.rename_merchant(merchant_id, name).await?;
        info!(merchant_id, name = %now, "Renamed merchant");
    }
    for argument in &config.merge_merchant {
        let (from_id, into_id) = split_pair(argument, "--merge-merchant", "FROM=INTO")?;
        if from_id == into_id {
            anyhow::bail!("--merge-merchant cannot merge merchant {} into itself", from_id);
        }
        client.merge_merchant(from_id, into_id).await?;
        info!(from_id, into_id, "Merged merchant");
    }
    for argument in &config.set_transaction_merchant {
        let (transaction_id, name) = split_pair(argument, "--set-transaction-merchant", "ID=NAME")?;
        client.update_transaction(transaction_id, Some(name), None).await?;
        info!(transaction_id, name, "Moved transaction to merchant");
    }
    if !config.split_transaction.is_empty() {
        let (_, categories) = client.fetch_categories().await?;
        for argument in &config.split_transaction {
            let (transaction_id, spec) = split_pair(argument, "--split-transaction", "ID=AMOUNT:CATEGORY;...")?;
            let mut parts = Vec::new();
            for part in spec.split(';') {
                let (amount, name) = part
                    .split_once(':')
                    .ok_or_else(|| anyhow::anyhow!("Split part '{}' should be AMOUNT:CATEGORY", part))?;
                let category_id = categories
                    .iter()
                    .find(|c| c["name"].as_str().is_some_and(|n| n.eq_ignore_ascii_case(name.trim())))
                    .and_then(|c| c["id"].as_str())
                    .ok_or_else(|| anyhow::anyhow!("No category named '{}'", name.trim()))?;
                parts.push((amount.trim().parse::<f64>()?, category_id.to_string()));
            }
            let children = client.split_transaction(transaction_id, &parts).await?;
            info!(transaction_id, parts = ?children, "Split transaction");
        }
    }
    for argument in &config.tag_transaction {
        let (transaction_id, tag) = split_pair(argument, "--tag-transaction", "ID=TAG")?;
        let tag_id = client.ensure_tag(tag, NEW_TAG_COLOR).await?;
        client.add_transaction_tag(transaction_id, &tag_id).await?;
        info!(transaction_id, tag, "Tagged transaction");
    }
    for argument in &config.untag_transaction {
        let (transaction_id, tag) = split_pair(argument, "--untag-transaction", "ID=TAG")?;
        let tag_id = client.ensure_tag(tag, NEW_TAG_COLOR).await?;
        client.remove_transaction_tag(transaction_id, &tag_id).await?;
        info!(transaction_id, tag, "Untagged transaction");
    }
    if !config.set_transaction_category.is_empty() {
        let (_, categories) = client.fetch_categories().await?;
        for argument in &config.set_transaction_category {
            let (transaction_id, name) = split_pair(argument, "--set-transaction-category", "ID=CATEGORY")?;
            let category_id = categories
                .iter()
                .find(|c| c["name"].as_str().is_some_and(|n| n.eq_ignore_ascii_case(name)))
                .and_then(|c| c["id"].as_str())
                .ok_or_else(|| anyhow::anyhow!("No category named '{}'", name))?;
            client.update_transaction(transaction_id, None, Some(category_id)).await?;
            info!(transaction_id, category = name, "Recategorized transaction");
        }
    }
    for match_name in &config.delete_merchant_rule {
        let deleted = client.delete_merchant_rules(match_name).await?;
        info!(match_name = %match_name, deleted, "Deleted merchant rules");
    }
    if !config.merchant_rule.is_empty() {
        let mut existing = client.merchant_rule_matches().await?;
        for argument in &config.merchant_rule {
            let (match_name, target_name) = split_pair(argument, "--merchant-rule", "MATCH=TARGET")?;
            if existing.contains(&match_name.to_lowercase()) {
                info!(match_name, "Rule already exists, skipping");
                continue;
            }
            client.create_merchant_rule(match_name, target_name).await?;
            existing.push(match_name.to_lowercase());
            info!(match_name, target_name, "Created merchant rule");
        }
    }
    if !config.limit_category_rule.is_empty() {
        let accounts = client.fetch_accounts().await?;
        for argument in &config.limit_category_rule {
            let (match_name, account) = split_pair(argument, "--limit-category-rule", "MATCH=ACCOUNT")?;
            let account_id = accounts
                .items
                .iter()
                .find(|a| a.display_name.as_deref().is_some_and(|n| n.eq_ignore_ascii_case(account)))
                .map(|a| a.id.as_str())
                .ok_or_else(|| anyhow::anyhow!("No account named '{}'", account))?;
            let changed = client.limit_category_rules(match_name, account_id).await?;
            info!(match_name, account, changed, "Limited category rules to one account");
        }
    }
    Ok(())
}

struct ScrapeSummary {
    accounts_count: usize,
    transactions_count: usize,
    holdings_count: usize,
    net_worth: Option<Decimal>,
}

async fn execute_scrape(
    config: &Config,
    client: &MonarchClient,
    db: Option<&Database>,
    run_id: Option<uuid::Uuid>,
) -> Result<ScrapeSummary> {
    if config.force_refresh {
        force_refresh_accounts(config, client).await?;
    }

    // Fetch accounts (always)
    info!("Fetching accounts...");
    let accounts = client.fetch_accounts().await?;
    info!(count = accounts.items.len(), "Fetched accounts");

    if config.verbose && !config.json {
        eprintln!("\n{}", "=== Accounts ===".bold());
        monarch_scraper::display::print_accounts_table(&accounts.items);
    }

    // Calculate net worth from accounts
    let mut net_worth = Decimal::ZERO;
    for account in &accounts.items {
        if account.include_in_net_worth.unwrap_or(true) && !account.is_hidden.unwrap_or(false) {
            let balance = account
                .display_balance
                .or(account.current_balance)
                .unwrap_or(Decimal::ZERO);
            if account.is_asset.unwrap_or(true) {
                net_worth += balance;
            } else {
                net_worth -= balance.abs();
            }
        }
    }

    // Store accounts in database
    if let (Some(db), Some(run_id)) = (db, run_id) {
        db.insert_accounts(run_id, &accounts.raw).await?;
    }

    // Fetch categories and tags (for database sync)
    if let Some(db) = db {
        info!("Fetching categories and tags...");
        let (groups, categories) = client.fetch_categories().await?;
        let tags = client.fetch_tags().await?;

        db.upsert_categories(&groups, &categories).await?;
        db.upsert_tags(&tags).await?;

        info!(
            category_groups = groups.len(),
            categories = categories.len(),
            tags = tags.len(),
            "Synced categories and tags"
        );
    }

    // Fetch transactions
    let transactions_count = if !config.skip_transactions {
        let since = Config::since(config.transaction_days);
        info!(since = ?since, "Fetching transactions...");
        let fetch = client.fetch_all_transactions(since).await?;
        let transactions = &fetch.transactions;
        info!(
            count = transactions.items.len(),
            total = fetch.total_count,
            "Fetched transactions"
        );

        if config.verbose && !config.json {
            eprintln!("\n{}", "=== Recent Transactions ===".bold());
            let display_txns: Vec<_> = transactions.items.iter().take(20).cloned().collect();
            monarch_scraper::display::print_transactions_table(&display_txns);
            if transactions.items.len() > 20 {
                eprintln!("... and {} more", transactions.items.len() - 20);
            }
        }

        // Store transactions in database
        if let (Some(db), Some(run_id)) = (db, run_id) {
            db.upsert_transactions(run_id, &transactions.raw).await?;

            // Only trust "not returned" to mean "deleted" when the fetch
            // provably covered everything in its window
            if fetch.is_complete() {
                let deleted = db.mark_missing_transactions_deleted(run_id, since).await?;
                if deleted > 0 {
                    info!(deleted, "Flagged transactions no longer in Monarch");
                }
                // Matching credits to purchases needs both sides, so it is
                // only redone when the whole history was fetched
                if since.is_none() {
                    let offsets = monarch_scraper::credits::offsets(&transactions.raw);
                    db.replace_credit_offsets(&offsets).await?;
                    info!(offsets = offsets.len(), "Matched statement credits to purchases");
                }

                // With the whole history in hand, merchants nothing points at
                // any more were merged away or deleted in Monarch
                if since.is_none() {
                    let pruned = db.prune_unused_merchants().await?;
                    if pruned > 0 {
                        info!(pruned, "Removed merchants no longer in Monarch");
                    }
                }
            } else {
                warn!(
                    fetched = transactions.items.len(),
                    total = fetch.total_count,
                    "Transaction list changed during fetch, skipping deletion check"
                );
            }
        }

        transactions.items.len()
    } else {
        0
    };

    // Fetch holdings
    let holdings_count = if !config.skip_holdings {
        info!("Fetching holdings...");
        let portfolio = client.fetch_holdings().await?;
        let positions_count: usize = portfolio
            .holdings
            .raw
            .iter()
            .filter_map(|holding| holding["holdings"].as_array())
            .map(Vec::len)
            .sum();
        info!(
            count = portfolio.holdings.items.len(),
            positions = positions_count,
            "Fetched holdings"
        );

        if config.verbose && !config.json {
            eprintln!("\n{}", "=== Holdings ===".bold());
            monarch_scraper::display::print_holdings_table(&portfolio.holdings.items);
        }

        // Store holdings, positions, tax lots and securities in database
        if let (Some(db), Some(run_id)) = (db, run_id) {
            db.insert_portfolio(run_id, &portfolio.holdings.raw, &portfolio.performance)
                .await?;
        }

        portfolio.holdings.items.len()
    } else {
        0
    };

    // Fetch recurring streams
    if !config.skip_recurring {
        info!("Fetching recurring streams...");
        let streams = client.fetch_recurring_streams().await?;
        info!(count = streams.len(), "Fetched recurring streams");

        if config.verbose && !config.json {
            eprintln!("\n{}", "=== Recurring Streams ===".bold());
            monarch_scraper::display::print_recurring_table(&streams);
        }

        // Store recurring streams in database
        if let (Some(db), Some(run_id)) = (db, run_id) {
            db.insert_recurring_streams(run_id, &streams).await?;
        }
    }

    if !config.skip_history {
        // Fetch aggregate snapshots (net worth history)
        let since = Config::since(config.history_days);
        info!(since = ?since, "Fetching net worth history...");
        let snapshots = client.fetch_aggregate_snapshots(since).await?;
        info!(count = snapshots.len(), "Fetched aggregate snapshots");

        // Store snapshots in database
        if let (Some(db), Some(run_id)) = (db, run_id) {
            db.upsert_aggregate_snapshots(run_id, &snapshots).await?;

            // Daily balance history is only worth fetching when it can be stored
            info!("Fetching account balance history...");
            let mut days = 0;
            let mut changed = 0;
            for account in &accounts.items {
                let balances = client.fetch_account_snapshots(&account.id).await?;
                days += balances.len();
                changed += db
                    .upsert_account_balances(run_id, &account.id, &balances)
                    .await?;
            }
            info!(
                accounts = accounts.items.len(),
                days,
                changed,
                "Synced account balance history"
            );
        }
    }

    // Budgets, goals, credit score and household are kept as whole documents.
    // They are supplementary, so one failing does not fail the run.
    if !config.skip_documents {
        if let (Some(db), Some(run_id)) = (db, run_id) {
            info!("Fetching documents...");
            for document in DOCUMENTS {
                match client.fetch_document(document).await {
                    Ok(payload) => db.upsert_document(run_id, document.name, &payload).await?,
                    Err(e) => warn!(
                        document = document.name,
                        error = %format!("{:#}", e),
                        "Could not fetch document"
                    ),
                }
            }
        }
    }

    // Print summary
    if !config.json {
        monarch_scraper::display::print_summary(
            &accounts.items,
            transactions_count,
            holdings_count,
            Some(net_worth),
        );
    }

    // JSON output if requested
    if config.json {
        let output = serde_json::json!({
            "accounts_count": accounts.items.len(),
            "transactions_count": transactions_count,
            "holdings_count": holdings_count,
            "net_worth": net_worth,
            "run_id": run_id,
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
    }

    Ok(ScrapeSummary {
        accounts_count: accounts.items.len(),
        transactions_count,
        holdings_count,
        net_worth: Some(net_worth),
    })
}

async fn send_notification(ntfy_url: &str, success: bool, summary: &ScrapeSummary) {
    let client = wreq::Client::new();

    let title = if success {
        "Monarch Scraper: Success"
    } else {
        "Monarch Scraper: Failed"
    };

    let body = format!(
        "Accounts: {}\nTransactions: {}\nHoldings: {}\nNet Worth: {}",
        summary.accounts_count,
        summary.transactions_count,
        summary.holdings_count,
        summary
            .net_worth
            .map(|nw| format!("${:.2}", nw))
            .unwrap_or_else(|| "N/A".to_string())
    );

    match client
        .post(ntfy_url)
        .header("Title", title)
        .header("Priority", if success { "low" } else { "high" })
        .header("Tags", if success { "white_check_mark" } else { "x" })
        .body(body)
        .send()
        .await
    {
        Ok(_) => info!("Notification sent"),
        Err(e) => warn!(error = %e, "Failed to send notification"),
    }
}

async fn send_failure_notification(ntfy_url: &str, error: &anyhow::Error) {
    let client = wreq::Client::new();

    match client
        .post(ntfy_url)
        .header("Title", "Monarch Scraper: Failed")
        .header("Priority", "high")
        .header("Tags", "x")
        .body(format!("Error: {:#}", error))
        .send()
        .await
    {
        Ok(_) => info!("Failure notification sent"),
        Err(e) => warn!(error = %e, "Failed to send notification"),
    }
}

fn mask_connection_string(url: &str) -> String {
    // Mask password in connection string for logging
    if let Some(at_pos) = url.find('@') {
        if let Some(colon_pos) = url[..at_pos].rfind(':') {
            let prefix = &url[..colon_pos + 1];
            let suffix = &url[at_pos..];
            return format!("{}****{}", prefix, suffix);
        }
    }
    url.to_string()
}
