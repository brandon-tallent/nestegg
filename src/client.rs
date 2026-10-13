//! Monarch Money GraphQL API client
//! Authentication via the web app's session cookie plus CSRF header, or the
//! legacy "Authorization: Token <token>" header when a token is supplied

use crate::config::Config;
use crate::models::*;
use crate::queries::*;
use anyhow::Result;
use chrono::{Datelike, Duration, Months, NaiveDate, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use wreq_util::Emulation;

const GRAPHQL_ENDPOINT: &str = "https://api.monarch.com/graphql";

/// Cookies the API needs for session auth; the rest of the browser's cookies
/// (analytics and the like) are not sent.
/// Transactions requested per page (the API accepts at least 5000, but large
/// pages of the full field set take tens of seconds to come back)
const TRANSACTIONS_PAGE_SIZE: usize = 500;

/// Start date used to ask for "all" history; predates Monarch itself
const HISTORY_EPOCH: &str = "2000-01-01";

/// Rule criterion operator for an exact match, as Monarch's rules store it
const RULE_OPERATOR_EQUALS: &str = "eq";
/// Transaction filter value that also returns transactions hidden from reports
const VISIBILITY_ALL: &str = "all_transactions";

/// Which stored document a response belongs to
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentKind {
    Budget,
    Goals,
    CreditScore,
    Household,
    HouseholdMembers,
    TransactionRules,
}

/// A whole API response mirrored into the `documents` table
#[derive(Debug, Clone, Copy)]
pub struct Document {
    pub kind: DocumentKind,
    /// Value of `documents.kind`
    pub name: &'static str,
    operation_name: &'static str,
    query: &'static str,
}

pub const DOCUMENTS: &[Document] = &[
    Document {
        kind: DocumentKind::Budget,
        name: "budget",
        operation_name: "Common_BudgetDataQuery",
        query: GET_BUDGET_QUERY,
    },
    Document {
        kind: DocumentKind::Goals,
        name: "goals",
        operation_name: "Common_SavingsGoalsWithThisMonthBalances",
        query: GET_GOALS_QUERY,
    },
    Document {
        kind: DocumentKind::CreditScore,
        name: "credit_score",
        operation_name: "Common_GetSpinwheelCreditScoreSnapshots",
        query: GET_CREDIT_SCORE_QUERY,
    },
    Document {
        kind: DocumentKind::Household,
        name: "household",
        operation_name: "Common_GetMyHousehold",
        query: GET_HOUSEHOLD_QUERY,
    },
    Document {
        kind: DocumentKind::HouseholdMembers,
        name: "household_members",
        operation_name: "Common_GetHouseholdMembers",
        query: GET_HOUSEHOLD_MEMBERS_QUERY,
    },
    Document {
        kind: DocumentKind::TransactionRules,
        name: "transaction_rules",
        operation_name: "Web_GetTransactionRules",
        query: GET_TRANSACTION_RULES_QUERY,
    },
];

/// First day of the month `offset` months after `date`, as YYYY-MM-DD
fn month_start(date: NaiveDate, offset: u32) -> String {
    let first = date.with_day(1).unwrap_or(date);
    first
        .checked_add_months(Months::new(offset))
        .unwrap_or(first)
        .format("%Y-%m-%d")
        .to_string()
}

/// Last day of the month containing `date`, as YYYY-MM-DD
fn month_end(date: NaiveDate) -> String {
    let first = date.with_day(1).unwrap_or(date);
    first
        .checked_add_months(Months::new(1))
        .and_then(|next| next.pred_opt())
        .unwrap_or(date)
        .format("%Y-%m-%d")
        .to_string()
}

/// Take the array at a JSON pointer out of a response
fn take_array(data: &mut Value, pointer: &str) -> Result<Vec<Value>> {
    match data.pointer_mut(pointer).map(Value::take) {
        Some(Value::Array(items)) => Ok(items),
        _ => anyhow::bail!("Response has no array at {}", pointer),
    }
}

/// How far ahead to look for the next occurrence of a recurring stream
/// (just over a year, so yearly streams are covered)
const RECURRING_LOOKAHEAD_DAYS: i64 = 370;

const SESSION_COOKIE: &str = "session_id";
const CSRF_COOKIE: &str = "csrftoken";
const CSRF_HEADER: &str = "x-csrftoken";

/// How requests are authenticated against the Monarch API
enum Auth {
    /// Legacy `Authorization: Token <token>` header
    Token(String),
    /// Session cookie with matching CSRF token (current web app behaviour)
    Session { cookie_header: String, csrf_token: String },
}

impl Auth {
    fn from_config(config: &Config) -> Result<Self> {
        if config.has_auth_token() {
            return Ok(Auth::Token(config.get_auth_token()?));
        }

        let cookies = config.load_cookies()?;
        let find = |name: &str| {
            cookies
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, v)| v.clone())
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "Cookie '{}' not found - run 'npm run refresh' in the browser directory to log in again",
                        name
                    )
                })
        };
        let session_id = find(SESSION_COOKIE)?;
        let csrf_token = find(CSRF_COOKIE)?;

        Ok(Auth::Session {
            cookie_header: format!(
                "{}={}; {}={}",
                SESSION_COOKIE, session_id, CSRF_COOKIE, csrf_token
            ),
            csrf_token,
        })
    }
}

pub struct MonarchClient {
    client: wreq::Client,
    auth: Auth,
    device_uuid: String,
    verbose: bool,
}

impl MonarchClient {
    pub fn new(config: &Config) -> Result<Self> {
        // Talk to the API the way the web app's browser does
        let client = wreq::Client::builder()
            .emulation(Emulation::Chrome149)
            .gzip(true)
            .build()?;

        let auth = Auth::from_config(config)?;
        let device_uuid = config.get_device_uuid()?;

        Ok(Self {
            client,
            auth,
            device_uuid,
            verbose: config.verbose,
        })
    }

    async fn execute_query<T>(&self, request: &GraphQLRequest) -> Result<T>
    where
        T: serde::de::DeserializeOwned,
    {
        if self.verbose {
            eprintln!(
                "Executing GraphQL: {} ({})",
                request.operation_name,
                serde_json::to_string(&request.variables)?
            );
        }

        let builder = self
            .client
            .post(GRAPHQL_ENDPOINT)
            .header("Accept", "application/json")
            .header("Content-Type", "application/json");
        let builder = match &self.auth {
            Auth::Token(token) => builder.header("Authorization", token),
            Auth::Session { cookie_header, csrf_token } => builder
                .header("Cookie", cookie_header)
                .header(CSRF_HEADER, csrf_token),
        };

        let response = builder
            .header("device-uuid", &self.device_uuid)
            .header("monarch-client", "monarch-core-web-app-graphql")
            .header("monarch-client-version", "v1.0.1068")
            .header("client-platform", "web")
            .header("Origin", "https://app.monarch.com")
            .header("Referer", "https://app.monarch.com/")
            .json(request)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!(
                "API request failed with status {}: {}",
                status,
                if body.len() > 500 { &body[..500] } else { &body }
            );
        }

        let response_text = response.text().await?;

        if self.verbose {
            eprintln!("Response length: {} bytes", response_text.len());
        }

        let graphql_response: GraphQLResponse<T> =
            serde_json::from_str(&response_text).map_err(|e| {
                anyhow::anyhow!(
                    "Failed to parse response: {}. Response preview: {}",
                    e,
                    if response_text.len() > 500 {
                        &response_text[..500]
                    } else {
                        &response_text
                    }
                )
            })?;

        if let Some(errors) = graphql_response.errors {
            if !errors.is_empty() {
                anyhow::bail!(
                    "GraphQL errors: {}",
                    errors
                        .iter()
                        .map(|e| e.message.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
        }

        graphql_response
            .data
            .ok_or_else(|| anyhow::anyhow!("No data in response"))
    }

    /// Run a query and return its `data` object untyped
    async fn execute_raw(
        &self,
        operation_name: &str,
        query: &str,
        variables: Value,
    ) -> Result<Value> {
        let request = GraphQLRequest {
            operation_name: operation_name.to_string(),
            variables,
            query: query.to_string(),
        };
        self.execute_query(&request).await
    }

    /// Fetch all accounts
    pub async fn fetch_accounts(&self) -> Result<Fetched<Account>> {
        let mut data = self
            .execute_raw("Web_GetAccountsPage", GET_ACCOUNTS_QUERY, json!({}))
            .await?;
        Ok(Fetched::parse(take_array(&mut data, "/accounts")?)?)
    }

    /// Fetch Monarch's full daily balance history for one account
    pub async fn fetch_account_snapshots(&self, account_id: &str) -> Result<Vec<Value>> {
        let mut data = self
            .execute_raw(
                "AccountDetails_getSnapshots",
                GET_ACCOUNT_SNAPSHOTS_QUERY,
                json!({ "id": account_id }),
            )
            .await?;
        take_array(&mut data, "/snapshots")
    }

    /// Fetch one page of transactions. `since` of `None` means all history.
    async fn fetch_transactions_page(
        &self,
        since: Option<NaiveDate>,
        limit: usize,
        offset: usize,
    ) -> Result<(Vec<Value>, i64)> {
        // Without an explicit visibility Monarch leaves out every account
        // whose transactions are hidden from reports (investments, loans)
        let mut filters = json!({ "transactionVisibility": VISIBILITY_ALL });
        if let Some(date) = since {
            filters["startDate"] = json!(date.format("%Y-%m-%d").to_string());
        }
        self.fetch_filtered_transactions_page(filters, limit, offset).await
    }

    /// Every transaction of one account, as Monarch returns them. Unlike the
    /// unfiltered list, this includes accounts whose transactions are hidden
    /// from reports (investment and loan accounts, usually).
    pub async fn fetch_account_transactions(&self, account_id: &str) -> Result<Vec<Value>> {
        let mut raw = Vec::new();
        loop {
            let (page, total) = self
                .fetch_filtered_transactions_page(
                    json!({ "accounts": [account_id], "transactionVisibility": VISIBILITY_ALL }),
                    TRANSACTIONS_PAGE_SIZE,
                    raw.len(),
                )
                .await?;
            let fetched_count = page.len();
            raw.extend(page);
            if fetched_count < TRANSACTIONS_PAGE_SIZE || raw.len() as i64 >= total {
                return Ok(raw);
            }
        }
    }

    async fn fetch_filtered_transactions_page(
        &self,
        filters: Value,
        limit: usize,
        offset: usize,
    ) -> Result<(Vec<Value>, i64)> {

        let mut data = self
            .execute_raw(
                "Web_GetTransactionsList",
                GET_TRANSACTIONS_QUERY,
                json!({
                    "offset": offset,
                    "limit": limit,
                    "filters": filters,
                    "orderBy": "date"
                }),
            )
            .await?;

        let total_count = data
            .pointer("/allTransactions/totalCount")
            .and_then(Value::as_i64)
            .ok_or_else(|| anyhow::anyhow!("Transactions response has no totalCount"))?;
        let results = take_array(&mut data, "/allTransactions/results")?;
        Ok((results, total_count))
    }

    /// Fetch every transaction dated on or after `since` (handles pagination).
    /// `since` of `None` fetches the whole history.
    pub async fn fetch_all_transactions(&self, since: Option<NaiveDate>) -> Result<TransactionFetch> {
        let mut raw = Vec::new();
        let mut total_count;
        let mut offset = 0;

        loop {
            let (page, total) = self
                .fetch_transactions_page(since, TRANSACTIONS_PAGE_SIZE, offset)
                .await?;
            total_count = total;

            if self.verbose {
                eprintln!(
                    "Fetched {} transactions (offset: {}, total: {})",
                    page.len(),
                    offset,
                    total_count
                );
            }

            let fetched_count = page.len();
            raw.extend(page);

            if fetched_count < TRANSACTIONS_PAGE_SIZE || raw.len() as i64 >= total_count {
                break;
            }

            offset += TRANSACTIONS_PAGE_SIZE;
        }

        Ok(TransactionFetch {
            transactions: Fetched::parse(raw)?,
            total_count,
        })
    }

    /// Fetch all category groups and categories, as (groups, categories)
    pub async fn fetch_categories(&self) -> Result<(Vec<Value>, Vec<Value>)> {
        let mut data = self
            .execute_raw(
                "Common_GetCategories",
                GET_CATEGORIES_QUERY,
                json!({ "includeSystemDisabledCategories": true }),
            )
            .await?;
        let groups = take_array(&mut data, "/categoryGroups")?;
        let categories = take_array(&mut data, "/categories")?;
        Ok((groups, categories))
    }

    /// Fetch all household tags
    pub async fn fetch_tags(&self) -> Result<Vec<Value>> {
        let mut data = self
            .execute_raw(
                "Common_GetHouseholdTransactionTags",
                GET_HOUSEHOLD_TAGS_QUERY,
                json!({}),
            )
            .await?;
        take_array(&mut data, "/householdTransactionTags")
    }

    /// Fetch the investment portfolio. Each holding is one security across
    /// all accounts and carries the per-account positions that make it up.
    pub async fn fetch_holdings(&self) -> Result<PortfolioFetch> {
        let mut data = self
            .execute_raw(
                "Web_GetPortfolio",
                GET_HOLDINGS_QUERY,
                json!({ "portfolioInput": {} }),
            )
            .await?;

        let nodes: Vec<Value> = take_array(&mut data, "/portfolio/aggregateHoldings/edges")?
            .into_iter()
            .map(|mut edge| edge["node"].take())
            .collect();

        // The return-over-time charts are large and not stored
        let mut performance = data["portfolio"]["performance"].take();
        if let Some(fields) = performance.as_object_mut() {
            fields.remove("historicalChart");
            fields.remove("benchmarks");
        }

        Ok(PortfolioFetch {
            holdings: Fetched::parse(nodes)?,
            performance,
        })
    }

    /// Fetch a whole response to keep as a document
    pub async fn fetch_document(&self, document: &Document) -> Result<Value> {
        let variables = match document.kind {
            // Monarch only returns the month containing startDate, whatever
            // the range, so the stored budget is the current month's
            DocumentKind::Budget => {
                let today = Utc::now().date_naive();
                json!({
                    "startDate": month_start(today, 0),
                    "endDate": month_end(today),
                })
            }
            _ => json!({}),
        };
        self.execute_raw(document.operation_name, document.query, variables)
            .await
    }

    /// Fetch recurring transaction streams, each annotated with its next
    /// upcoming occurrence within the lookahead window
    pub async fn fetch_recurring_streams(&self) -> Result<Vec<RecurringStream>> {
        let today = Utc::now();
        let request = GraphQLRequest {
            operation_name: "Common_GetRecurringStreams".to_string(),
            variables: serde_json::json!({
                "startDate": today.format("%Y-%m-%d").to_string(),
                "endDate": (today + Duration::days(RECURRING_LOOKAHEAD_DAYS))
                    .format("%Y-%m-%d")
                    .to_string(),
            }),
            query: GET_RECURRING_STREAMS_QUERY.to_string(),
        };

        let data: RecurringStreamsData = self.execute_query(&request).await?;
        let mut streams = Vec::with_capacity(data.recurring_transaction_streams.len());
        for mut edge in data.recurring_transaction_streams {
            let raw = edge["stream"].take();
            let mut stream = RecurringStream::deserialize(&raw)?;
            stream.raw = Some(raw);
            streams.push(stream);
        }

        // ISO dates sort lexicographically, so the smallest is the next occurrence
        for stream in &mut streams {
            let next = data
                .recurring_transaction_items
                .iter()
                .filter(|item| item.stream.id == stream.id && item.date.is_some())
                .min_by(|a, b| a.date.cmp(&b.date));
            if let Some(item) = next {
                stream.next_forecasted_date = item.date.clone();
                stream.account_id = item.account.as_ref().map(|a| a.id.clone());
            }
        }

        Ok(streams)
    }

    /// Fetch daily net worth history from `since` onwards (`None` for all of it)
    pub async fn fetch_aggregate_snapshots(&self, since: Option<NaiveDate>) -> Result<Vec<Value>> {
        let start_date = since
            .map(|date| date.format("%Y-%m-%d").to_string())
            .unwrap_or_else(|| HISTORY_EPOCH.to_string());

        let mut data = self
            .execute_raw(
                "Common_GetAggregateSnapshots",
                GET_AGGREGATE_SNAPSHOTS_QUERY,
                json!({
                    "filters": {
                        "startDate": start_date,
                        "endDate": null,
                        "accountType": null,
                        "useAdaptiveGranularity": false
                    }
                }),
            )
            .await?;
        take_array(&mut data, "/aggregateSnapshots")
    }

    /// Ask Monarch to re-sync every connected account with its institution.
    /// Returns the id of the refresh operation, if Monarch started one.
    pub async fn force_refresh_all_accounts(&self) -> Result<Option<String>> {
        let request = GraphQLRequest {
            operation_name: "Common_ForceRefreshAccountsMutation".to_string(),
            variables: serde_json::json!({}),
            query: FORCE_REFRESH_ALL_ACCOUNTS_MUTATION.to_string(),
        };

        let data: ForceRefreshAllAccountsData = self.execute_query(&request).await?;
        let payload = data
            .force_refresh_all_accounts
            .ok_or_else(|| anyhow::anyhow!("Force refresh returned no result"))?;

        if let Some(error) = payload.errors {
            anyhow::bail!(
                "Force refresh rejected: {} ({})",
                error.message.unwrap_or_else(|| "no message".to_string()),
                error.code.unwrap_or_else(|| "no code".to_string())
            );
        }
        if payload.success != Some(true) {
            anyhow::bail!("Force refresh was not accepted by Monarch");
        }

        Ok(payload.force_refresh_operation_id)
    }

    /// Fetch the progress of a force refresh operation
    pub async fn fetch_force_refresh_operation(
        &self,
        operation_id: &str,
    ) -> Result<Option<ForceRefreshOperation>> {
        let request = GraphQLRequest {
            operation_name: "Common_ForceRefreshOperationQuery".to_string(),
            variables: serde_json::json!({ "id": operation_id }),
            query: GET_FORCE_REFRESH_OPERATION_QUERY.to_string(),
        };

        let data: ForceRefreshOperationData = self.execute_query(&request).await?;
        Ok(data.force_refresh_operation)
    }

    /// Whether any account is still syncing with its institution
    pub async fn has_accounts_syncing(&self) -> Result<bool> {
        let request = GraphQLRequest {
            operation_name: "Common_ForceRefreshAccountsQuery".to_string(),
            variables: serde_json::json!({}),
            query: HAS_ACCOUNTS_SYNCING_QUERY.to_string(),
        };

        let data: HasAccountsSyncingData = self.execute_query(&request).await?;
        Ok(data.has_accounts_syncing)
    }

    /// Merge one merchant into another: every transaction of `from_id` is
    /// moved to `into_id` and `from_id` is deleted. This cannot be undone.
    pub async fn merge_merchant(&self, from_id: &str, into_id: &str) -> Result<()> {
        let data = self
            .execute_raw(
                "Common_DeleteMerchant",
                DELETE_MERCHANT_MUTATION,
                json!({ "merchantId": from_id, "moveToId": into_id }),
            )
            .await?;
        if data.pointer("/deleteMerchant/success").and_then(Value::as_bool) != Some(true) {
            anyhow::bail!("Monarch did not confirm merging merchant {} into {}", from_id, into_id);
        }
        Ok(())
    }

    /// Rename a merchant, returning the name Monarch now reports
    pub async fn rename_merchant(&self, merchant_id: &str, name: &str) -> Result<String> {
        let data = self
            .execute_raw(
                "Common_UpdateMerchant",
                UPDATE_MERCHANT_MUTATION,
                json!({ "input": { "merchantId": merchant_id, "name": name } }),
            )
            .await?;
        if let Some(message) = data.pointer("/updateMerchant/errors/message").and_then(Value::as_str) {
            anyhow::bail!("Monarch rejected renaming merchant {}: {}", merchant_id, message);
        }
        data.pointer("/updateMerchant/merchant/name")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| anyhow::anyhow!("Monarch did not rename merchant {}: {}", merchant_id, data["updateMerchant"]))
    }

    /// Merchant names that already have a rule renaming them (lowercased).
    /// A rule that matches a name only to set its category does not count:
    /// transactions it catches would still keep the old merchant name.
    pub async fn merchant_rule_matches(&self) -> Result<Vec<String>> {
        let mut data = self
            .execute_raw("Web_GetTransactionRules", GET_TRANSACTION_RULES_QUERY, json!({}))
            .await?;
        Ok(take_array(&mut data, "/transactionRules")?
            .iter()
            .filter(|rule| !rule["setMerchantAction"].is_null())
            .filter_map(|rule| rule["merchantCriteria"].as_array())
            .flatten()
            .filter(|criterion| criterion["operator"] == RULE_OPERATOR_EQUALS)
            .filter_map(|criterion| criterion["value"].as_str())
            .map(str::to_lowercase)
            .collect())
    }

    /// Create a rule that files future transactions whose merchant is exactly
    /// `match_name` under the merchant `target_name` instead. Existing
    /// transactions are left alone.
    pub async fn create_merchant_rule(&self, match_name: &str, target_name: &str) -> Result<()> {
        let data = self
            .execute_raw(
                "Common_CreateTransactionRuleMutationV2",
                CREATE_TRANSACTION_RULE_MUTATION,
                json!({
                    "input": {
                        "merchantCriteria": [{ "operator": RULE_OPERATOR_EQUALS, "value": match_name.to_lowercase() }],
                        "setMerchantAction": target_name,
                        "applyToExistingTransactions": false,
                    }
                }),
            )
            .await?;
        let errors = &data["createTransactionRuleV2"]["errors"];
        if !errors.is_null() {
            anyhow::bail!("Monarch rejected the rule for '{}': {}", match_name, errors);
        }
        Ok(())
    }

    /// Limit every rule that only sets the category of merchants named exactly
    /// `match_name` to the account `account_id`, so the same merchant name on
    /// another account (a card payment named after the card) is left alone.
    /// Existing transactions are not touched. Returns how many rules changed.
    pub async fn limit_category_rules(&self, match_name: &str, account_id: &str) -> Result<usize> {
        let mut data = self
            .execute_raw("Web_GetTransactionRules", GET_TRANSACTION_RULES_QUERY, json!({}))
            .await?;
        let wanted = match_name.to_lowercase();
        let rules = take_array(&mut data, "/transactionRules")?;
        let mut changed = 0;
        for rule in &rules {
            let criteria = rule["merchantCriteria"].as_array().map(Vec::as_slice).unwrap_or_default();
            let matches = criteria.iter().any(|c| {
                c["operator"] == RULE_OPERATOR_EQUALS && c["value"].as_str().is_some_and(|v| v.to_lowercase() == wanted)
            });
            // The update replaces the whole rule, so only touch the plain shape
            // this can rebuild exactly: one merchant, one category, nothing else
            let plain = criteria.len() == 1
                && ["setMerchantAction", "addTagsAction", "amountCriteria", "categoryIds", "accountIds",
                    "originalStatementCriteria", "merchantNameCriteria", "reviewStatusAction"]
                    .iter()
                    .all(|field| rule[*field].is_null());
            let (Some(id), Some(category_id)) = (rule["id"].as_str(), rule["setCategoryAction"]["id"].as_str()) else {
                continue;
            };
            if !matches || !plain {
                continue;
            }
            let result = self
                .execute_raw(
                    "Common_UpdateTransactionRuleMutationV2",
                    UPDATE_TRANSACTION_RULE_MUTATION,
                    json!({
                        "input": {
                            "id": id,
                            "merchantCriteriaUseOriginalStatement": rule["merchantCriteriaUseOriginalStatement"],
                            "merchantCriteria": [{ "operator": RULE_OPERATOR_EQUALS, "value": wanted }],
                            "accountIds": [account_id],
                            "setCategoryAction": category_id,
                            "setHideFromReportsAction": rule["setHideFromReportsAction"],
                            "applyToExistingTransactions": false,
                        }
                    }),
                )
                .await?;
            let errors = &result["updateTransactionRuleV2"]["errors"];
            if !errors.is_null() {
                anyhow::bail!("Monarch rejected the change to rule {}: {}", id, errors);
            }
            changed += 1;
        }
        Ok(changed)
    }

    /// Create a rule that files future transactions whose bank statement text
    /// is exactly `statement` under the category `category_id`, on one account
    /// when `account_id` is given. Existing transactions are left alone.
    /// Returns false, creating nothing, when a rule already matches that text.
    pub async fn create_statement_category_rule(
        &self, statement: &str, category_id: &str, account_id: Option<&str>,
    ) -> Result<bool> {
        let mut data = self
            .execute_raw("Web_GetTransactionRules", GET_TRANSACTION_RULES_QUERY, json!({}))
            .await?;
        let wanted = statement.to_lowercase();
        let exists = take_array(&mut data, "/transactionRules")?
            .iter()
            .filter_map(|rule| rule["originalStatementCriteria"].as_array())
            .flatten()
            .any(|c| c["value"].as_str().is_some_and(|v| v.to_lowercase() == wanted));
        if exists {
            return Ok(false);
        }
        let mut input = json!({
            "originalStatementCriteria": [{ "operator": RULE_OPERATOR_EQUALS, "value": wanted }],
            "setCategoryAction": category_id,
            "applyToExistingTransactions": false,
        });
        if let Some(id) = account_id {
            input["accountIds"] = json!([id]);
        }
        let result = self
            .execute_raw(
                "Common_CreateTransactionRuleMutationV2",
                CREATE_TRANSACTION_RULE_MUTATION,
                json!({ "input": input }),
            )
            .await?;
        let errors = &result["createTransactionRuleV2"]["errors"];
        if !errors.is_null() {
            anyhow::bail!("Monarch rejected the rule for '{}': {}", statement, errors);
        }
        Ok(true)
    }

    /// Merchants whose name matches `search`, as (id, name, transaction count).
    /// Includes merchants no transaction uses, which the mirror never sees.
    pub async fn search_merchants(&self, search: &str) -> Result<Vec<(String, String, i64)>> {
        let mut data = self
            .execute_raw(
                "Web_GetMerchantSettingsPage",
                SEARCH_MERCHANTS_QUERY,
                json!({ "search": search }),
            )
            .await?;
        Ok(take_array(&mut data, "/merchants")?
            .iter()
            .map(|m| {
                (
                    m["id"].as_str().unwrap_or_default().to_string(),
                    m["name"].as_str().unwrap_or_default().to_string(),
                    m["transactionCount"].as_i64().unwrap_or(0),
                )
            })
            .collect())
    }

    /// Change a transaction's merchant (by name) and/or category (by id)
    pub async fn update_transaction(
        &self,
        transaction_id: &str,
        merchant_name: Option<&str>,
        category_id: Option<&str>,
    ) -> Result<()> {
        self.update_transaction_confirmed(transaction_id, merchant_name, category_id).await?;
        Ok(())
    }

    /// The canonical fields returned by Monarch after an edit, for the write API.
    pub async fn update_transaction_confirmed(
        &self, transaction_id: &str, merchant_name: Option<&str>, category_id: Option<&str>,
    ) -> Result<Value> {
        let mut input = json!({ "id": transaction_id });
        if let Some(name) = merchant_name {
            input["name"] = json!(name);
        }
        if let Some(id) = category_id {
            input["category"] = json!(id);
        }
        let data = self
            .execute_raw("Common_UpdateTransaction", UPDATE_TRANSACTION_MUTATION, json!({ "input": input }))
            .await?;
        if data.pointer("/updateTransaction/transaction/id").and_then(Value::as_str).is_none() {
            anyhow::bail!(
                "Monarch did not update transaction {}: {}",
                transaction_id,
                data["updateTransaction"]
            );
        }
        if data["updateTransaction"]["errors"].as_array().is_some_and(|errors| !errors.is_empty()) {
            anyhow::bail!("Monarch reported errors while updating transaction {}", transaction_id);
        }
        let transaction = data["updateTransaction"]["transaction"].clone();
        if transaction["id"].as_str() != Some(transaction_id) {
            anyhow::bail!("Monarch returned a different transaction after an edit");
        }
        Ok(transaction)
    }

    /// Delete every rule that renames merchants named exactly `match_name`.
    /// Returns how many were deleted.
    pub async fn delete_merchant_rules(&self, match_name: &str) -> Result<usize> {
        let mut data = self
            .execute_raw("Web_GetTransactionRules", GET_TRANSACTION_RULES_QUERY, json!({}))
            .await?;
        let wanted = match_name.to_lowercase();
        let ids: Vec<String> = take_array(&mut data, "/transactionRules")?
            .iter()
            .filter(|rule| !rule["setMerchantAction"].is_null())
            .filter(|rule| {
                rule["merchantCriteria"].as_array().is_some_and(|criteria| {
                    criteria.iter().any(|c| {
                        c["operator"] == RULE_OPERATOR_EQUALS
                            && c["value"].as_str().is_some_and(|v| v.to_lowercase() == wanted)
                    })
                })
            })
            .filter_map(|rule| rule["id"].as_str().map(str::to_string))
            .collect();
        for id in &ids {
            let result = self
                .execute_raw(
                    "Common_DeleteTransactionRule",
                    DELETE_TRANSACTION_RULE_MUTATION,
                    json!({ "id": id }),
                )
                .await?;
            // Monarch reports `deleted: false` even when the rule is removed, so
            // only an explicit error counts as failure
            if !result["deleteTransactionRule"]["errors"].is_null() {
                anyhow::bail!("Monarch did not delete rule {}: {}", id, result["deleteTransactionRule"]);
            }
        }
        Ok(ids.len())
    }

    /// Delete a transaction from Monarch. This cannot be undone.
    pub async fn delete_transaction(&self, transaction_id: &str) -> Result<()> {
        let data = self
            .execute_raw(
                "Common_DeleteTransactionMutation",
                DELETE_TRANSACTION_MUTATION,
                json!({ "input": { "transactionId": transaction_id } }),
            )
            .await?;
        let result = &data["deleteTransaction"];
        if !result["errors"].is_null() || result.is_null() {
            anyhow::bail!("Monarch did not delete transaction {}: {}", transaction_id, result);
        }
        Ok(())
    }

    /// Id of the tag with this name, creating it if it does not exist yet
    pub async fn ensure_tag(&self, name: &str, color: &str) -> Result<String> {
        if let Some(id) = self
            .fetch_tags()
            .await?
            .iter()
            .find(|tag| tag["name"].as_str().is_some_and(|n| n.eq_ignore_ascii_case(name)))
            .and_then(|tag| tag["id"].as_str())
        {
            return Ok(id.to_string());
        }
        let data = self
            .execute_raw(
                "Common_CreateTransactionTag",
                CREATE_TAG_MUTATION,
                json!({ "input": { "name": name, "color": color } }),
            )
            .await?;
        data.pointer("/createTransactionTag/tag/id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| anyhow::anyhow!("Monarch did not create tag '{}': {}", name, data["createTransactionTag"]))
    }

    /// Add a tag to a transaction, keeping the tags it already has
    pub async fn add_transaction_tag(&self, transaction_id: &str, tag_id: &str) -> Result<()> {
        let current = self
            .execute_raw("GetTransactionDrawer", GET_TRANSACTION_QUERY, json!({ "id": transaction_id }))
            .await?;
        let mut tag_ids: Vec<String> = current
            .pointer("/getTransaction/tags")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow::anyhow!("Transaction {} not found", transaction_id))?
            .iter()
            .filter_map(|tag| tag["id"].as_str().map(str::to_string))
            .collect();
        if tag_ids.iter().any(|id| id == tag_id) {
            return Ok(());
        }
        tag_ids.push(tag_id.to_string());

        let data = self
            .execute_raw(
                "Web_SetTransactionTags",
                SET_TRANSACTION_TAGS_MUTATION,
                json!({ "input": { "transactionId": transaction_id, "tagIds": tag_ids } }),
            )
            .await?;
        if data.pointer("/setTransactionTags/transaction/id").is_none() {
            anyhow::bail!("Monarch did not tag transaction {}: {}", transaction_id, data["setTransactionTags"]);
        }
        Ok(())
    }

    /// Remove a tag from a transaction, keeping its other tags
    pub async fn remove_transaction_tag(&self, transaction_id: &str, tag_id: &str) -> Result<()> {
        let current = self
            .execute_raw("GetTransactionDrawer", GET_TRANSACTION_QUERY, json!({ "id": transaction_id }))
            .await?;
        let tag_ids: Vec<String> = current
            .pointer("/getTransaction/tags")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow::anyhow!("Transaction {} not found", transaction_id))?
            .iter()
            .filter_map(|tag| tag["id"].as_str().map(str::to_string))
            .collect();
        if !tag_ids.iter().any(|id| id == tag_id) {
            return Ok(());
        }
        let remaining: Vec<&String> = tag_ids.iter().filter(|id| id.as_str() != tag_id).collect();

        let data = self
            .execute_raw(
                "Web_SetTransactionTags",
                SET_TRANSACTION_TAGS_MUTATION,
                json!({ "input": { "transactionId": transaction_id, "tagIds": remaining } }),
            )
            .await?;
        if data.pointer("/setTransactionTags/transaction/id").is_none() {
            anyhow::bail!("Monarch did not untag transaction {}: {}", transaction_id, data["setTransactionTags"]);
        }
        Ok(())
    }

    /// Split a transaction into parts, each `(amount, category id)`. The parts
    /// must add up to the transaction's amount; they keep its merchant.
    /// Returns the ids of the parts.
    pub async fn split_transaction(&self, transaction_id: &str, parts: &[(f64, String)]) -> Result<Vec<String>> {
        let current = self
            .execute_raw("GetTransactionDrawer", GET_TRANSACTION_QUERY, json!({ "id": transaction_id }))
            .await?;
        let merchant = current
            .pointer("/getTransaction/merchant/name")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("Transaction {} not found", transaction_id))?;
        let split_data: Vec<Value> = parts
            .iter()
            .map(|(amount, category_id)| json!({ "merchantName": merchant, "amount": amount, "categoryId": category_id }))
            .collect();

        let data = self
            .execute_raw(
                "Common_SplitTransactionMutation",
                SPLIT_TRANSACTION_MUTATION,
                json!({ "input": { "transactionId": transaction_id, "splitData": split_data } }),
            )
            .await?;
        let children = data
            .pointer("/updateTransactionSplit/transaction/splitTransactions")
            .and_then(Value::as_array)
            .filter(|children| !children.is_empty())
            .ok_or_else(|| {
                anyhow::anyhow!("Monarch did not split transaction {}: {}", transaction_id, data["updateTransactionSplit"])
            })?;
        Ok(children.iter().filter_map(|c| c["id"].as_str().map(str::to_string)).collect())
    }
}
