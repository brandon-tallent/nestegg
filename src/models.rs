//! Data models for Monarch Money GraphQL API responses

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

// =============================================================================
// GraphQL Request/Response Wrappers
// =============================================================================

#[derive(Debug, Serialize)]
pub struct GraphQLRequest {
    #[serde(rename = "operationName")]
    pub operation_name: String,
    pub variables: serde_json::Value,
    pub query: String,
}

#[derive(Debug, Deserialize)]
pub struct GraphQLResponse<T> {
    pub data: Option<T>,
    pub errors: Option<Vec<GraphQLError>>,
}

#[derive(Debug, Deserialize)]
pub struct GraphQLError {
    pub message: String,
    pub locations: Option<Vec<ErrorLocation>>,
    pub path: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub struct ErrorLocation {
    pub line: i32,
    pub column: i32,
}

/// Objects fetched from the API, both parsed and exactly as returned.
/// `items[i]` was parsed from `raw[i]`.
#[derive(Debug, Clone)]
pub struct Fetched<T> {
    pub items: Vec<T>,
    pub raw: Vec<serde_json::Value>,
}

impl<T: serde::de::DeserializeOwned> Fetched<T> {
    pub fn parse(raw: Vec<serde_json::Value>) -> Result<Self, serde_json::Error> {
        let items = raw
            .iter()
            .map(|value| T::deserialize(value))
            .collect::<Result<Vec<T>, _>>()?;
        Ok(Self { items, raw })
    }
}

/// Result of fetching a window of transactions
#[derive(Debug, Clone)]
pub struct TransactionFetch {
    pub transactions: Fetched<Transaction>,
    /// Count Monarch reported for the window
    pub total_count: i64,
}

impl TransactionFetch {
    /// Every transaction Monarch reported was received exactly once, so
    /// anything absent from the fetch is genuinely gone from Monarch
    pub fn is_complete(&self) -> bool {
        let unique: std::collections::HashSet<&str> = self
            .transactions
            .items
            .iter()
            .map(|t| t.id.as_str())
            .collect();
        unique.len() as i64 == self.total_count
            && self.transactions.items.len() as i64 == self.total_count
    }
}

/// Investment portfolio: one holding per security, plus overall performance
#[derive(Debug, Clone)]
pub struct PortfolioFetch {
    pub holdings: Fetched<Holding>,
    pub performance: serde_json::Value,
}

// =============================================================================
// Account Types
// =============================================================================

#[derive(Debug, Deserialize)]
pub struct AccountsData {
    pub accounts: Vec<Account>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Account {
    pub id: String,
    #[serde(rename = "displayName")]
    pub display_name: Option<String>,
    #[serde(rename = "syncDisabled")]
    pub sync_disabled: Option<bool>,
    #[serde(rename = "deactivatedAt")]
    pub deactivated_at: Option<String>,
    #[serde(rename = "isHidden")]
    pub is_hidden: Option<bool>,
    #[serde(rename = "isAsset")]
    pub is_asset: Option<bool>,
    #[serde(rename = "includeInNetWorth")]
    pub include_in_net_worth: Option<bool>,
    #[serde(rename = "currentBalance")]
    pub current_balance: Option<Decimal>,
    #[serde(rename = "displayBalance")]
    pub display_balance: Option<Decimal>,
    #[serde(rename = "signedBalance")]
    pub signed_balance: Option<Decimal>,
    #[serde(rename = "availableBalance")]
    pub available_balance: Option<Decimal>,
    #[serde(rename = "type")]
    pub account_type: Option<AccountType>,
    pub subtype: Option<AccountSubtype>,
    pub credential: Option<Credential>,
    pub institution: Option<Institution>,
    #[serde(rename = "logoUrl")]
    pub logo_url: Option<String>,
    #[serde(rename = "isManual")]
    pub is_manual: Option<bool>,
    #[serde(rename = "lastSyncedAt")]
    pub last_synced_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AccountType {
    pub name: Option<String>,
    pub display: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AccountSubtype {
    pub name: Option<String>,
    pub display: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Credential {
    pub id: String,
    #[serde(rename = "updateRequired")]
    pub update_required: Option<bool>,
    #[serde(rename = "disconnectedFromDataProvider")]
    pub disconnected_from_data_provider: Option<bool>,
    pub institution: Option<Institution>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Institution {
    pub id: String,
    pub name: Option<String>,
    #[serde(rename = "primaryColor")]
    pub primary_color: Option<String>,
    pub url: Option<String>,
    #[serde(rename = "logoUrl")]
    pub logo_url: Option<String>,
}

// =============================================================================
// Transaction Types
// =============================================================================

#[derive(Debug, Deserialize)]
pub struct TransactionsData {
    #[serde(rename = "allTransactions")]
    pub all_transactions: TransactionConnection,
}

#[derive(Debug, Deserialize)]
pub struct TransactionConnection {
    pub results: Vec<Transaction>,
    #[serde(rename = "totalCount")]
    pub total_count: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Transaction {
    pub id: String,
    pub amount: Decimal,
    pub pending: Option<bool>,
    pub date: String,
    #[serde(rename = "originalDate")]
    pub original_date: Option<String>,
    #[serde(rename = "hideFromReports")]
    pub hide_from_reports: Option<bool>,
    #[serde(rename = "needsReview")]
    pub needs_review: Option<bool>,
    #[serde(rename = "reviewedAt")]
    pub reviewed_at: Option<String>,
    #[serde(rename = "reviewedByUser")]
    pub reviewed_by_user: Option<TransactionUser>,
    #[serde(rename = "plaidName")]
    pub plaid_name: Option<String>,
    pub notes: Option<String>,
    #[serde(rename = "isSplitTransaction")]
    pub is_split_transaction: Option<bool>,
    #[serde(rename = "splitTransactions")]
    pub split_transactions: Option<Vec<SplitTransaction>>,
    #[serde(rename = "originalTransaction")]
    pub original_transaction: Option<Box<OriginalTransaction>>,
    #[serde(rename = "attachments")]
    pub attachments: Option<Vec<Attachment>>,
    pub account: TransactionAccount,
    pub category: Option<TransactionCategory>,
    pub merchant: Option<Merchant>,
    pub tags: Option<Vec<Tag>>,
    pub goal: Option<TransactionGoal>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TransactionUser {
    pub id: String,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SplitTransaction {
    pub id: String,
    pub amount: Decimal,
    #[serde(rename = "merchantName")]
    pub merchant_name: Option<String>,
    pub category: Option<TransactionCategory>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OriginalTransaction {
    pub id: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Attachment {
    pub id: String,
    #[serde(rename = "filename")]
    pub filename: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TransactionAccount {
    pub id: String,
    #[serde(rename = "displayName")]
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TransactionCategory {
    pub id: String,
    pub name: Option<String>,
    pub icon: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Merchant {
    pub id: String,
    pub name: Option<String>,
    #[serde(rename = "logoUrl")]
    pub logo_url: Option<String>,
    #[serde(rename = "transactionsCount")]
    pub transactions_count: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Tag {
    pub id: String,
    pub name: String,
    pub color: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TransactionGoal {
    pub id: String,
    pub name: Option<String>,
}

// =============================================================================
// Category Types
// =============================================================================

#[derive(Debug, Deserialize)]
pub struct CategoriesData {
    pub categories: Vec<Category>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Category {
    pub id: String,
    pub name: String,
    pub icon: Option<String>,
    pub order: Option<i32>,
    #[serde(rename = "systemCategory")]
    pub system_category: Option<String>,
    #[serde(rename = "isSystemCategory")]
    pub is_system_category: Option<bool>,
    #[serde(rename = "isDisabled")]
    pub is_disabled: Option<bool>,
    pub group: Option<CategoryGroup>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CategoryGroup {
    pub id: String,
    pub name: Option<String>,
    #[serde(rename = "type")]
    pub group_type: Option<String>,
}

// =============================================================================
// Tag Types (Household Tags)
// =============================================================================

#[derive(Debug, Deserialize)]
pub struct HouseholdTagsData {
    #[serde(rename = "householdTransactionTags")]
    pub household_transaction_tags: Vec<Tag>,
}

// =============================================================================
// Holdings Types (Web_GetPortfolio response)
// =============================================================================

/// Top-level response wrapper for Web_GetPortfolio query
#[derive(Debug, Deserialize)]
pub struct PortfolioData {
    pub portfolio: Portfolio,
}

/// Portfolio object containing performance summary and holdings
#[derive(Debug, Deserialize)]
pub struct Portfolio {
    pub performance: PortfolioPerformance,
    #[serde(rename = "aggregateHoldings")]
    pub aggregate_holdings: AggregateHoldings,
}

/// Portfolio-level performance metrics
#[derive(Debug, Clone, Deserialize)]
pub struct PortfolioPerformance {
    #[serde(rename = "totalValue")]
    pub total_value: Option<Decimal>,
    #[serde(rename = "totalBasis")]
    pub total_basis: Option<Decimal>,
    #[serde(rename = "totalChangePercent")]
    pub total_change_percent: Option<Decimal>,
    #[serde(rename = "totalChangeDollars")]
    pub total_change_dollars: Option<Decimal>,
}

/// GraphQL connection type for holdings (edges/node pattern)
#[derive(Debug, Deserialize)]
pub struct AggregateHoldings {
    pub edges: Vec<HoldingEdge>,
}

/// GraphQL edge wrapper for a holding node
#[derive(Debug, Deserialize)]
pub struct HoldingEdge {
    pub node: Holding,
}

/// Individual investment holding
#[derive(Debug, Clone, Deserialize)]
pub struct Holding {
    pub id: String,
    pub quantity: Option<Decimal>,
    /// Cost basis of the holding
    #[serde(rename = "costBasis")]
    pub basis: Option<Decimal>,
    /// Current total market value
    #[serde(rename = "totalValue")]
    pub total_value: Option<Decimal>,
    /// Dollar change in security price
    #[serde(rename = "securityPriceChangeDollars")]
    pub security_price_change_dollars: Option<Decimal>,
    /// Percent change in security price
    #[serde(rename = "securityPriceChangePercent")]
    pub security_price_change_percent: Option<Decimal>,
    pub security: Option<Security>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Security {
    pub id: String,
    pub name: Option<String>,
    /// Ticker symbol (e.g., "AAPL", "VTI")
    pub ticker: Option<String>,
    #[serde(rename = "currentPrice")]
    pub current_price: Option<Decimal>,
}

// =============================================================================
// Recurring Stream Types
// =============================================================================

#[derive(Debug, Deserialize)]
pub struct RecurringStreamsData {
    #[serde(rename = "recurringTransactionStreams")]
    pub recurring_transaction_streams: Vec<serde_json::Value>,
    #[serde(rename = "recurringTransactionItems")]
    pub recurring_transaction_items: Vec<RecurringItem>,
}

/// An upcoming occurrence of a recurring stream
#[derive(Debug, Deserialize)]
pub struct RecurringItem {
    pub stream: RecurringStreamRef,
    pub date: Option<String>,
    pub account: Option<RecurringItemAccount>,
}

#[derive(Debug, Deserialize)]
pub struct RecurringStreamRef {
    pub id: String,
}

#[derive(Debug, Deserialize)]
pub struct RecurringItemAccount {
    pub id: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RecurringStream {
    pub id: String,
    pub frequency: Option<String>,
    #[serde(rename = "isActive")]
    pub is_active: Option<bool>,
    /// e.g. approved, automatic_approved, pending
    #[serde(rename = "reviewStatus")]
    pub review_status: Option<String>,
    pub merchant: Option<Merchant>,
    #[serde(rename = "name")]
    pub display_name: Option<String>,
    #[serde(rename = "logoUrl")]
    pub logo_url: Option<String>,
    pub amount: Option<Decimal>,
    /// e.g. expense, income, credit_card
    #[serde(rename = "recurringType")]
    pub stream_type: Option<String>,
    /// Date of the next upcoming occurrence (filled from recurringTransactionItems)
    #[serde(skip)]
    pub next_forecasted_date: Option<String>,
    /// Account of the next upcoming occurrence (filled from recurringTransactionItems)
    #[serde(skip)]
    pub account_id: Option<String>,
    /// The stream exactly as returned by the API
    #[serde(skip)]
    pub raw: Option<serde_json::Value>,
}

impl RecurringStream {
    pub fn is_approved(&self) -> bool {
        self.review_status
            .as_deref()
            .is_some_and(|status| status.ends_with("approved"))
    }
}

// =============================================================================
// Aggregate Snapshot Types (Net Worth History)
// =============================================================================

#[derive(Debug, Deserialize)]
pub struct AggregateSnapshotsData {
    #[serde(rename = "aggregateSnapshots")]
    pub aggregate_snapshots: Vec<AggregateSnapshot>,
}

/// Historical balance snapshot for net worth tracking
#[derive(Debug, Clone, Deserialize)]
pub struct AggregateSnapshot {
    pub date: String,
    /// Net worth (assets - liabilities)
    pub balance: Option<Decimal>,
    #[serde(rename = "assetsBalance")]
    pub assets_balance: Option<Decimal>,
    #[serde(rename = "liabilitiesBalance")]
    pub liabilities_balance: Option<Decimal>,
}

impl AggregateSnapshot {
    /// Get net worth, preferring the direct balance field
    pub fn net_worth(&self) -> Option<Decimal> {
        // Use direct balance if available, otherwise calculate
        self.balance.or_else(|| {
            match (self.assets_balance, self.liabilities_balance) {
                (Some(assets), Some(liabilities)) => Some(assets - liabilities),
                (Some(assets), None) => Some(assets),
                _ => None,
            }
        })
    }
}

// =============================================================================
// Force Refresh Types
// =============================================================================

#[derive(Debug, Deserialize)]
pub struct ForceRefreshAllAccountsData {
    #[serde(rename = "forceRefreshAllAccounts")]
    pub force_refresh_all_accounts: Option<ForceRefreshAllAccountsPayload>,
}

#[derive(Debug, Deserialize)]
pub struct ForceRefreshAllAccountsPayload {
    pub success: Option<bool>,
    #[serde(rename = "forceRefreshOperationId")]
    pub force_refresh_operation_id: Option<String>,
    pub errors: Option<PayloadError>,
}

#[derive(Debug, Deserialize)]
pub struct PayloadError {
    pub message: Option<String>,
    pub code: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ForceRefreshOperationData {
    #[serde(rename = "forceRefreshOperation")]
    pub force_refresh_operation: Option<ForceRefreshOperation>,
}

#[derive(Debug, Deserialize)]
pub struct ForceRefreshOperation {
    pub id: String,
    pub state: Option<String>,
    #[serde(rename = "completedAccountCount")]
    pub completed_account_count: Option<i64>,
    #[serde(rename = "totalAccountCount")]
    pub total_account_count: Option<i64>,
    #[serde(default)]
    pub accounts: Vec<ForceRefreshAccountProgress>,
}

impl ForceRefreshOperation {
    /// Every account in the operation has finished (successfully or not)
    pub fn is_finished(&self) -> bool {
        match (self.completed_account_count, self.total_account_count) {
            (Some(completed), Some(total)) => completed >= total,
            _ => false,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct ForceRefreshAccountProgress {
    #[serde(rename = "accountId")]
    pub account_id: Option<String>,
    pub state: Option<String>,
    #[serde(rename = "newTransactionCount")]
    pub new_transaction_count: Option<i64>,
    #[serde(rename = "updatedTransactionCount")]
    pub updated_transaction_count: Option<i64>,
    #[serde(rename = "timedOut")]
    pub timed_out: Option<bool>,
    #[serde(rename = "errorMessage")]
    pub error_message: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct HasAccountsSyncingData {
    #[serde(rename = "hasAccountsSyncing")]
    pub has_accounts_syncing: bool,
}
