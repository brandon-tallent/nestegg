-- Monarch Money Scraper Database Schema
--
-- Applied automatically by the CLI on every connect, so every statement must
-- be idempotent (CREATE ... IF NOT EXISTS / CREATE OR REPLACE VIEW).
--
-- Three kinds of tables:
--   * Mirrors    - one row per Monarch object, upserted on every run and kept
--                  current (transactions, securities, categories, tags,
--                  merchants, daily balances, net worth history).
--   * Snapshots  - one row per object per run, so history accumulates
--                  (accounts, holdings, positions, recurring_streams).
--   * Documents  - whole API responses kept as JSONB, one version per day
--                  (budgets, goals, credit score, household).
--
-- Mirror and snapshot rows carry a `raw` JSONB column holding the object
-- exactly as Monarch returned it, so fields without a dedicated column are
-- still queryable.

-- =============================================================================
-- RUNS TABLE - Tracks each execution
-- =============================================================================
CREATE TABLE IF NOT EXISTS runs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    run_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    finished_at TIMESTAMPTZ,
    status VARCHAR(20) NOT NULL DEFAULT 'running', -- running, success, failed
    force_refresh BOOLEAN NOT NULL DEFAULT false,  -- a re-sync of all accounts was requested first
    accounts_count INTEGER,
    transactions_count INTEGER,
    holdings_count INTEGER,
    net_worth DECIMAL(15,2),
    error_message TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_runs_run_at ON runs(run_at DESC);
CREATE INDEX IF NOT EXISTS idx_runs_status ON runs(status);

-- =============================================================================
-- CATEGORY_GROUPS / CATEGORIES / TAGS / MERCHANTS - Reference data (mirrors)
-- =============================================================================
CREATE TABLE IF NOT EXISTS category_groups (
    id VARCHAR(50) PRIMARY KEY,
    name TEXT,
    group_type VARCHAR(50),            -- income, expense, transfer
    group_order INTEGER,
    raw JSONB,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS categories (
    id VARCHAR(50) PRIMARY KEY,
    name VARCHAR(200) NOT NULL,
    icon VARCHAR(50),
    group_id VARCHAR(50),
    group_name VARCHAR(100),
    group_type VARCHAR(50),
    category_order INTEGER,
    is_system_category BOOLEAN DEFAULT false,
    is_disabled BOOLEAN DEFAULT false,
    exclude_from_budget BOOLEAN DEFAULT false,
    raw JSONB,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS tags (
    id VARCHAR(50) PRIMARY KEY,
    name VARCHAR(200) NOT NULL,
    color VARCHAR(20),
    tag_order INTEGER,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS merchants (
    id VARCHAR(50) PRIMARY KEY,
    name TEXT,
    logo_url TEXT,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- =============================================================================
-- MERCHANT_MERGES TABLE - Merchants merged away in Monarch through this tool
-- =============================================================================
-- A merge deletes the source merchant in Monarch and cannot be undone there,
-- so this is the only record of what the original grouping was.
CREATE TABLE IF NOT EXISTS merchant_merges (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    from_merchant_id VARCHAR(50) NOT NULL,
    from_name TEXT NOT NULL,
    from_transactions INTEGER,
    into_merchant_id VARCHAR(50) NOT NULL,
    into_name TEXT NOT NULL,
    merged_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- =============================================================================
-- ACCOUNTS TABLE - All account types with balances (snapshot per run)
-- =============================================================================
CREATE TABLE IF NOT EXISTS accounts (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    run_id UUID NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    account_id VARCHAR(50) NOT NULL,
    display_name VARCHAR(200),
    mask VARCHAR(20),
    account_type VARCHAR(50),          -- depository, brokerage, credit, loan, etc.
    account_subtype VARCHAR(50),
    institution_id VARCHAR(50),
    institution_name VARCHAR(200),
    institution_logo_url TEXT,
    current_balance DECIMAL(15,2),
    available_balance DECIMAL(15,2),
    display_balance DECIMAL(15,2),
    signed_balance DECIMAL(15,2),      -- negative for liabilities
    credit_limit DECIMAL(15,2),
    include_in_net_worth BOOLEAN DEFAULT true,
    is_hidden BOOLEAN DEFAULT false,
    is_asset BOOLEAN DEFAULT true,
    is_manual BOOLEAN DEFAULT false,
    is_taxable BOOLEAN,
    sync_disabled BOOLEAN DEFAULT false,
    data_provider VARCHAR(50),         -- plaid, mx, finicity, ...
    credential_id VARCHAR(50),
    credential_update_required BOOLEAN,
    holdings_count INTEGER,
    account_order INTEGER,
    deactivated_at TIMESTAMPTZ,
    last_synced_at TIMESTAMPTZ,
    raw JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_accounts_run_id ON accounts(run_id);
CREATE INDEX IF NOT EXISTS idx_accounts_account_id ON accounts(account_id);
CREATE INDEX IF NOT EXISTS idx_accounts_type ON accounts(account_type);

-- =============================================================================
-- ACCOUNT_DAILY_BALANCES - Monarch's own daily balance history per account
-- =============================================================================
CREATE TABLE IF NOT EXISTS account_daily_balances (
    account_id VARCHAR(50) NOT NULL,
    balance_date DATE NOT NULL,
    balance DECIMAL(15,2),             -- signed: negative for liabilities
    run_id UUID REFERENCES runs(id) ON DELETE SET NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (account_id, balance_date)
);

CREATE INDEX IF NOT EXISTS idx_account_daily_balances_date ON account_daily_balances(balance_date DESC);

-- =============================================================================
-- TRANSACTIONS TABLE - Full transaction history (mirror, one row per transaction)
-- =============================================================================
-- Accounts whose transactions Monarch hides from reports (investments, loans)
-- are mirrored too, with hidden_by_account set; leave them out of spending.
-- run_id is the last run that saw the transaction. A transaction that Monarch
-- no longer returns is kept with deleted_at set rather than removed.
CREATE TABLE IF NOT EXISTS transactions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    run_id UUID REFERENCES runs(id) ON DELETE SET NULL,
    transaction_id VARCHAR(50) NOT NULL UNIQUE,
    account_id VARCHAR(50) NOT NULL,
    date DATE NOT NULL,
    original_date DATE,
    amount DECIMAL(15,2) NOT NULL,
    pending BOOLEAN DEFAULT false,
    notes TEXT,
    original_name TEXT,
    plaid_name TEXT,
    data_provider_description TEXT,
    merchant_id VARCHAR(50),
    merchant_name TEXT,
    category_id VARCHAR(50),
    category_name VARCHAR(200),
    category_group_type VARCHAR(50),   -- income, expense, transfer
    is_reviewed BOOLEAN DEFAULT false,
    review_status VARCHAR(50),
    needs_review BOOLEAN DEFAULT false,
    reviewed_at TIMESTAMPTZ,
    is_split BOOLEAN DEFAULT false,
    has_splits BOOLEAN DEFAULT false,
    split_parent_id VARCHAR(50),
    is_recurring BOOLEAN DEFAULT false,
    is_manual BOOLEAN DEFAULT false,
    hide_from_reports BOOLEAN DEFAULT false,
    hidden_by_account BOOLEAN DEFAULT false,
    goal_id VARCHAR(50),
    owner_user_id VARCHAR(50),
    attachments_count INTEGER DEFAULT 0,
    monarch_created_at TIMESTAMPTZ,
    monarch_updated_at TIMESTAMPTZ,
    raw JSONB,
    first_seen_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_seen_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    changed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),  -- last time any field differed
    deleted_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_transactions_run_id ON transactions(run_id);
CREATE INDEX IF NOT EXISTS idx_transactions_account_id ON transactions(account_id);
CREATE INDEX IF NOT EXISTS idx_transactions_date ON transactions(date DESC);
CREATE INDEX IF NOT EXISTS idx_transactions_category_id ON transactions(category_id);
CREATE INDEX IF NOT EXISTS idx_transactions_merchant ON transactions(merchant_name);
CREATE INDEX IF NOT EXISTS idx_transactions_merchant_id ON transactions(merchant_id);

-- =============================================================================
-- CREDIT_OFFSETS TABLE - Statement credits matched to the purchase they refund
-- =============================================================================
-- Rebuilt on every full sync. A purchase's true cost is its amount plus the
-- amounts of the credits pointing at it.
CREATE TABLE IF NOT EXISTS credit_offsets (
    credit_transaction_id VARCHAR(50) PRIMARY KEY,
    purchase_transaction_id VARCHAR(50) NOT NULL,
    amount DECIMAL(15,2) NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_credit_offsets_purchase ON credit_offsets(purchase_transaction_id);

-- =============================================================================
-- TRANSACTION_TAGS TABLE - Many-to-many relationship for transaction tags
-- =============================================================================
CREATE TABLE IF NOT EXISTS transaction_tags (
    transaction_id VARCHAR(50) NOT NULL,
    tag_id VARCHAR(50) NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (transaction_id, tag_id)
);

CREATE INDEX IF NOT EXISTS idx_transaction_tags_tag_id ON transaction_tags(tag_id);

-- =============================================================================
-- SECURITIES TABLE - Instruments referenced by holdings (mirror)
-- =============================================================================
CREATE TABLE IF NOT EXISTS securities (
    id VARCHAR(50) PRIMARY KEY,
    ticker VARCHAR(50),
    name TEXT,
    security_type VARCHAR(50),         -- equity, etf, mutual_fund, cash, ...
    security_type_display VARCHAR(100),
    asset_class VARCHAR(50),
    current_price DECIMAL(20,6),
    current_price_updated_at TIMESTAMPTZ,
    closing_price DECIMAL(20,6),
    raw JSONB,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_securities_ticker ON securities(ticker);

-- =============================================================================
-- HOLDINGS TABLE - One row per security across all accounts (snapshot per run)
-- =============================================================================
CREATE TABLE IF NOT EXISTS holdings (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    run_id UUID NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    holding_id VARCHAR(50) NOT NULL,
    account_id VARCHAR(50),            -- unused: see positions for per-account rows
    security_id VARCHAR(50),
    security_name VARCHAR(200),
    security_type VARCHAR(50),
    ticker VARCHAR(50),
    cusip VARCHAR(20),
    quantity DECIMAL(24,8),
    price DECIMAL(20,6),
    value DECIMAL(15,2),
    cost_basis DECIMAL(15,2),
    gain_loss DECIMAL(15,2),           -- value - cost_basis
    gain_loss_percent DECIMAL(12,4),
    price_change_dollars DECIMAL(20,6),  -- one-day security price move
    price_change_percent DECIMAL(12,4),
    currency VARCHAR(10) DEFAULT 'USD',
    is_cash BOOLEAN DEFAULT false,
    last_synced_at TIMESTAMPTZ,
    raw JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_holdings_run_id ON holdings(run_id);
CREATE INDEX IF NOT EXISTS idx_holdings_holding_id ON holdings(holding_id);
CREATE INDEX IF NOT EXISTS idx_holdings_ticker ON holdings(ticker);

-- =============================================================================
-- POSITIONS TABLE - One row per security per account (snapshot per run)
-- =============================================================================
CREATE TABLE IF NOT EXISTS positions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    run_id UUID NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    position_id VARCHAR(50) NOT NULL,          -- Monarch holding id
    aggregate_holding_id VARCHAR(50),          -- holdings.holding_id it rolls up into
    account_id VARCHAR(50),
    security_id VARCHAR(50),
    ticker VARCHAR(50),
    name TEXT,
    security_type VARCHAR(50),
    security_type_display VARCHAR(100),
    quantity DECIMAL(24,8),
    closing_price DECIMAL(20,6),
    closing_price_updated_at TIMESTAMPTZ,
    value DECIMAL(15,2),
    cost_basis DECIMAL(15,2),
    user_cost_basis DECIMAL(15,2),             -- cost basis entered by hand
    gain_loss DECIMAL(15,2),
    gain_loss_percent DECIMAL(12,4),
    tax_lots_count INTEGER DEFAULT 0,
    raw JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_positions_run_id ON positions(run_id);
CREATE INDEX IF NOT EXISTS idx_positions_account_id ON positions(account_id);
CREATE INDEX IF NOT EXISTS idx_positions_security_id ON positions(security_id);
CREATE INDEX IF NOT EXISTS idx_positions_ticker ON positions(ticker);

-- =============================================================================
-- POSITION_TAX_LOTS TABLE - Purchase lots behind a position (mirror)
-- =============================================================================
-- run_id is the last run that saw the lot; lots sold off stop being updated.
CREATE TABLE IF NOT EXISTS position_tax_lots (
    lot_id VARCHAR(50) PRIMARY KEY,
    position_id VARCHAR(50) NOT NULL,
    account_id VARCHAR(50),
    security_id VARCHAR(50),
    acquisition_date DATE,
    acquisition_quantity DECIMAL(24,8),
    cost_basis_per_unit DECIMAL(20,6),
    monarch_created_at TIMESTAMPTZ,
    run_id UUID REFERENCES runs(id) ON DELETE SET NULL,
    first_seen_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_seen_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_position_tax_lots_position_id ON position_tax_lots(position_id);
CREATE INDEX IF NOT EXISTS idx_position_tax_lots_run_id ON position_tax_lots(run_id);

-- =============================================================================
-- PORTFOLIO_SNAPSHOTS TABLE - Portfolio-level performance (snapshot per run)
-- =============================================================================
CREATE TABLE IF NOT EXISTS portfolio_snapshots (
    run_id UUID PRIMARY KEY REFERENCES runs(id) ON DELETE CASCADE,
    total_value DECIMAL(15,2),
    total_change_dollars DECIMAL(15,2),
    total_change_percent DECIMAL(12,4),
    one_day_change_percent DECIMAL(12,4),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- =============================================================================
-- RECURRING_STREAMS TABLE - Recurring income/expenses (snapshot per run)
-- =============================================================================
CREATE TABLE IF NOT EXISTS recurring_streams (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    run_id UUID NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    stream_id VARCHAR(50) NOT NULL,
    frequency VARCHAR(50),             -- monthly, yearly, semimonthly_start_mid, etc.
    stream_type VARCHAR(50),           -- expense, income, credit_card
    review_status VARCHAR(50),         -- approved, automatic_approved, pending
    is_active BOOLEAN DEFAULT true,
    is_approved BOOLEAN DEFAULT false,
    merchant_id VARCHAR(50),
    merchant_name VARCHAR(200),
    display_name VARCHAR(200),
    logo_url TEXT,
    amount DECIMAL(15,2),
    next_expected_date DATE,
    last_date DATE,
    category_id VARCHAR(50),
    account_id VARCHAR(50),
    raw JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_recurring_streams_run_id ON recurring_streams(run_id);
CREATE INDEX IF NOT EXISTS idx_recurring_streams_stream_id ON recurring_streams(stream_id);
CREATE INDEX IF NOT EXISTS idx_recurring_streams_type ON recurring_streams(stream_type);

-- =============================================================================
-- AGGREGATE_SNAPSHOTS TABLE - Daily net worth history (mirror, one row per day)
-- =============================================================================
CREATE TABLE IF NOT EXISTS aggregate_snapshots (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    run_id UUID REFERENCES runs(id) ON DELETE SET NULL,
    snapshot_date DATE NOT NULL UNIQUE,
    assets_total DECIMAL(15,2),
    liabilities_total DECIMAL(15,2),
    net_worth DECIMAL(15,2),
    investments_total DECIMAL(15,2),
    cash_total DECIMAL(15,2),
    other_assets DECIMAL(15,2),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- =============================================================================
-- DOCUMENTS TABLE - Whole API responses, one version per kind per day
-- =============================================================================
-- kind: budget, goals, credit_score, household, household_members, transaction_rules
CREATE TABLE IF NOT EXISTS documents (
    kind VARCHAR(50) NOT NULL,
    as_of_date DATE NOT NULL DEFAULT CURRENT_DATE,
    payload JSONB NOT NULL,
    run_id UUID REFERENCES runs(id) ON DELETE SET NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (kind, as_of_date)
);

-- =============================================================================
-- VIEWS - Convenient access to latest data
-- =============================================================================

-- View for latest successful run
CREATE OR REPLACE VIEW latest_run AS
SELECT * FROM runs
WHERE status = 'success'
ORDER BY run_at DESC
LIMIT 1;

-- View for accounts from most recent successful run
CREATE OR REPLACE VIEW latest_accounts AS
SELECT a.*
FROM accounts a
WHERE a.run_id = (SELECT id FROM latest_run);

-- View for every transaction Monarch currently has
CREATE OR REPLACE VIEW latest_transactions AS
SELECT t.*
FROM transactions t
WHERE t.deleted_at IS NULL;

-- View for holdings (per security) from most recent successful run
CREATE OR REPLACE VIEW latest_holdings AS
SELECT h.*
FROM holdings h
WHERE h.run_id = (SELECT id FROM latest_run);

-- View for positions (per security per account) from most recent successful run
CREATE OR REPLACE VIEW latest_positions AS
SELECT
    p.*,
    a.display_name AS account_name,
    a.institution_name,
    a.account_subtype,
    a.is_taxable
FROM positions p
LEFT JOIN latest_accounts a ON a.account_id = p.account_id
WHERE p.run_id = (SELECT id FROM latest_run);

-- View for tax lots still held as of the most recent successful run
CREATE OR REPLACE VIEW latest_tax_lots AS
SELECT l.*
FROM position_tax_lots l
WHERE l.run_id = (SELECT id FROM latest_run);

-- View for recurring streams from most recent successful run
CREATE OR REPLACE VIEW latest_recurring_streams AS
SELECT rs.*
FROM recurring_streams rs
WHERE rs.run_id = (SELECT id FROM latest_run);

-- View for the newest version of each stored document
CREATE OR REPLACE VIEW latest_documents AS
SELECT DISTINCT ON (kind) kind, as_of_date, payload, run_id, updated_at
FROM documents
ORDER BY kind, as_of_date DESC;

-- View for net worth history from aggregate snapshots
CREATE OR REPLACE VIEW net_worth_history AS
SELECT
    snapshot_date,
    assets_total,
    liabilities_total,
    net_worth,
    investments_total,
    cash_total
FROM aggregate_snapshots
ORDER BY snapshot_date DESC;

-- View for account balance history as observed by each run
CREATE OR REPLACE VIEW account_balance_history AS
SELECT
    a.account_id,
    a.display_name,
    a.institution_name,
    a.account_type,
    r.run_at,
    a.current_balance,
    a.display_balance
FROM accounts a
JOIN runs r ON a.run_id = r.id
WHERE r.status = 'success'
ORDER BY a.account_id, r.run_at DESC;

-- View for monthly spending by category
CREATE OR REPLACE VIEW monthly_spending_by_category AS
SELECT
    date_trunc('month', t.date) AS month,
    t.category_name,
    COUNT(*) AS transaction_count,
    SUM(t.amount) AS total_amount
FROM latest_transactions t
WHERE t.amount < 0  -- expenses are negative
  AND NOT COALESCE(t.hidden_by_account, false)  -- not a brokerage's buys or a loan's payments
GROUP BY date_trunc('month', t.date), t.category_name
ORDER BY month DESC, total_amount ASC;

-- =============================================================================
-- EQUITY - Private company holdings as captured from Carta
-- =============================================================================
-- Rebuilt whole from each capture (browser/src/carta.ts, loaded with
-- --load-carta), since a capture is the complete current state. Only
-- equity_snapshots accumulates: one row per company per capture day.

CREATE TABLE IF NOT EXISTS equity_issuers (
    issuer_id BIGINT PRIMARY KEY,
    name VARCHAR(200) NOT NULL,
    -- Latest fair market value per common share (the 409A price); NULL when
    -- Carta reports none, as for a company that has since been restructured
    fmv DECIMAL(20,6),
    held_since DATE,
    -- Cash paid in for the shares on record, as Carta totals it
    cash_cost DECIMAL(15,2),
    captured_at TIMESTAMPTZ NOT NULL,
    raw JSONB,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS equity_grants (
    grant_id BIGINT PRIMARY KEY,
    issuer_id BIGINT NOT NULL,
    label VARCHAR(50) NOT NULL,
    -- ISO or NSO as granted; a grant over the yearly ISO limit vests partly as each
    option_type VARCHAR(20),
    -- How Carta's options table describes it: "ISO/NSO Split" for such a grant
    granted_as VARCHAR(50),
    -- The stock the options buy: Common, ...
    stock_type VARCHAR(50),
    issue_date DATE,
    expiration_date DATE,
    quantity BIGINT NOT NULL,
    vested BIGINT NOT NULL DEFAULT 0,
    exercised BIGINT NOT NULL DEFAULT 0,
    -- Vested and not yet exercised
    exercisable BIGINT NOT NULL DEFAULT 0,
    exercise_price DECIMAL(20,6) NOT NULL,
    status VARCHAR(50),
    is_canceled BOOLEAN NOT NULL DEFAULT FALSE,
    vesting_start DATE,
    vesting_end DATE,
    vesting_schedule TEXT,
    -- "Yes, 25% vests at 1 year"
    cliff TEXT,
    -- The schedule spelled out as in the grant agreement
    vesting_terms TEXT,
    -- Empty when vesting does not accelerate
    acceleration TEXT,
    -- Whether unvested options may be exercised
    early_exercise BOOLEAN,
    -- ISOs that have lost their status, as after leaving the company
    iso_disqualified BOOLEAN,
    -- NONE_ELIGIBLE unless an early exercise has an 83(b) election to file
    form_83b_status VARCHAR(50),
    raw JSONB,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_equity_grants_issuer ON equity_grants(issuer_id);

CREATE TABLE IF NOT EXISTS equity_vesting_events (
    grant_id BIGINT NOT NULL,
    vest_date DATE NOT NULL,
    quantity BIGINT NOT NULL,
    cumulative BIGINT NOT NULL,
    -- How the tranche divides between ISO and NSO, where Carta says
    iso_quantity BIGINT,
    nso_quantity BIGINT,
    -- How many of the tranche's options have been exercised
    exercised BIGINT,
    has_vested BOOLEAN NOT NULL DEFAULT FALSE,
    -- "cliff" or "interval"
    kind VARCHAR(50),
    PRIMARY KEY (grant_id, vest_date)
);

CREATE TABLE IF NOT EXISTS equity_certificates (
    certificate_id BIGINT PRIMARY KEY,
    issuer_id BIGINT NOT NULL,
    label VARCHAR(50) NOT NULL,
    share_class VARCHAR(100),
    stock_type VARCHAR(50),
    -- Whether a share certificate was issued, rather than a book entry
    is_certificated BOOLEAN,
    issue_date DATE,
    -- When the shares were first acquired; survives a certificate being split
    acquired_date DATE,
    quantity BIGINT NOT NULL,
    cost DECIMAL(15,2),
    -- Outstanding, Secondary (sold in a tender), Transferred (split), ...
    status VARCHAR(50),
    is_canceled BOOLEAN NOT NULL DEFAULT FALSE,
    -- For shares bought by exercising options: ISO or NSO, and the grant's label
    exercise_type VARCHAR(20),
    exercised_from VARCHAR(50),
    fmv_at_acquisition DECIMAL(20,6),
    raw JSONB,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_equity_certificates_issuer ON equity_certificates(issuer_id);

CREATE TABLE IF NOT EXISTS equity_exercises (
    certificate_id BIGINT PRIMARY KEY,
    issuer_id BIGINT NOT NULL,
    grant_id BIGINT,
    grant_label VARCHAR(50),
    certificate_label VARCHAR(50),
    option_type VARCHAR(20),
    exercised_at TIMESTAMPTZ,
    quantity BIGINT NOT NULL,
    exercise_price DECIMAL(20,6),
    cost DECIMAL(15,2),
    -- Tax withheld on top of the cost, as for an NSO exercised while employed
    taxes DECIMAL(15,2),
    early_exercise BOOLEAN,
    -- Fair market value per share on the exercise date; the spread over the
    -- exercise price is what the exercise is taxed on
    fmv DECIMAL(20,6),
    payment_method VARCHAR(50),
    status VARCHAR(50),
    raw JSONB
);

CREATE TABLE IF NOT EXISTS equity_tenders (
    tender_id BIGINT PRIMARY KEY,
    issuer_name VARCHAR(200),
    name VARCHAR(300),
    status VARCHAR(100),
    -- secondary_sale, ...
    kind VARCHAR(50),
    start_date DATE,
    end_date DATE,
    settlement_date DATE,
    -- Per share, as Carta rounds it; proceeds / shares_sold is exact
    price DECIMAL(20,6),
    shares_sold BIGINT,
    proceeds DECIMAL(15,2),
    raw JSONB
);

-- Columns added since the tables above were first created
ALTER TABLE equity_grants ADD COLUMN IF NOT EXISTS granted_as VARCHAR(50);
ALTER TABLE equity_grants ADD COLUMN IF NOT EXISTS stock_type VARCHAR(50);
ALTER TABLE equity_grants ADD COLUMN IF NOT EXISTS cliff TEXT;
ALTER TABLE equity_grants ADD COLUMN IF NOT EXISTS vesting_terms TEXT;
ALTER TABLE equity_grants ADD COLUMN IF NOT EXISTS acceleration TEXT;
ALTER TABLE equity_grants ADD COLUMN IF NOT EXISTS early_exercise BOOLEAN;
ALTER TABLE equity_grants ADD COLUMN IF NOT EXISTS iso_disqualified BOOLEAN;
ALTER TABLE equity_grants ADD COLUMN IF NOT EXISTS form_83b_status VARCHAR(50);
ALTER TABLE equity_vesting_events ADD COLUMN IF NOT EXISTS exercised BIGINT;
ALTER TABLE equity_certificates ADD COLUMN IF NOT EXISTS stock_type VARCHAR(50);
ALTER TABLE equity_certificates ADD COLUMN IF NOT EXISTS is_certificated BOOLEAN;
ALTER TABLE equity_exercises ADD COLUMN IF NOT EXISTS taxes DECIMAL(15,2);
ALTER TABLE equity_exercises ADD COLUMN IF NOT EXISTS early_exercise BOOLEAN;
ALTER TABLE equity_tenders ADD COLUMN IF NOT EXISTS kind VARCHAR(50);

-- What the holdings came to at each capture, to chart value over time
CREATE TABLE IF NOT EXISTS equity_snapshots (
    issuer_id BIGINT NOT NULL,
    snapshot_date DATE NOT NULL,
    fmv DECIMAL(20,6),
    shares BIGINT NOT NULL,
    options_vested BIGINT NOT NULL,
    options_unvested BIGINT NOT NULL,
    shares_value DECIMAL(15,2) NOT NULL,
    -- Options at their spread over the exercise price, never below zero
    vested_options_value DECIMAL(15,2) NOT NULL,
    unvested_options_value DECIMAL(15,2) NOT NULL,
    cost_to_exercise_vested DECIMAL(15,2) NOT NULL,
    PRIMARY KEY (issuer_id, snapshot_date)
);
