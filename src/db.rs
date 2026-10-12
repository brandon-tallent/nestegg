//! Database operations for Monarch Money data
//!
//! Most writes hand Postgres the objects exactly as Monarch returned them (a
//! JSON array) and let SQL pick out the columns. That keeps one statement per
//! batch and guarantees the `raw` column matches the typed columns.

use crate::models::*;
use anyhow::{Context, Result};
use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde_json::Value;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use uuid::Uuid;

/// Schema applied on every connect; every statement in it is idempotent
const SCHEMA_SQL: &str = include_str!("../sql/schema.sql");

/// Rows sent to Postgres per statement when writing large sets
const WRITE_BATCH_SIZE: usize = 1000;

pub struct Database {
    pool: PgPool,
}

/// Serialize a batch of raw API objects for a `$n::jsonb` parameter
fn json_array(items: &[Value]) -> Result<String> {
    Ok(serde_json::to_string(items)?)
}

impl Database {
    pub async fn connect(database_url: &str) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(database_url)
            .await?;

        sqlx::raw_sql(SCHEMA_SQL)
            .execute(&pool)
            .await
            .context("Failed to apply database schema")?;

        Ok(Self { pool })
    }

    /// Create a new run record
    pub async fn create_run(&self, force_refresh: bool) -> Result<Uuid> {
        let row: (Uuid,) = sqlx::query_as(
            r#"
            INSERT INTO runs (status, force_refresh)
            VALUES ('running', $1)
            RETURNING id
            "#,
        )
        .bind(force_refresh)
        .fetch_one(&self.pool)
        .await?;

        Ok(row.0)
    }

    /// Update run status to success
    pub async fn complete_run(
        &self,
        run_id: Uuid,
        accounts_count: i32,
        transactions_count: i32,
        holdings_count: i32,
        net_worth: Option<Decimal>,
    ) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE runs
            SET status = 'success',
                finished_at = NOW(),
                accounts_count = $2,
                transactions_count = $3,
                holdings_count = $4,
                net_worth = $5
            WHERE id = $1
            "#,
        )
        .bind(run_id)
        .bind(accounts_count)
        .bind(transactions_count)
        .bind(holdings_count)
        .bind(net_worth)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Update run status to failed
    pub async fn fail_run(&self, run_id: Uuid, error_message: &str) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE runs
            SET status = 'failed',
                finished_at = NOW(),
                error_message = $2
            WHERE id = $1
            "#,
        )
        .bind(run_id)
        .bind(error_message)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Insert this run's snapshot of every account
    pub async fn insert_accounts(&self, run_id: Uuid, accounts: &[Value]) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO accounts (
                run_id, account_id, display_name, mask, account_type, account_subtype,
                institution_id, institution_name, institution_logo_url,
                current_balance, display_balance, signed_balance, credit_limit,
                include_in_net_worth, is_hidden, is_asset, is_manual, is_taxable,
                sync_disabled, data_provider, credential_id, credential_update_required,
                holdings_count, account_order, deactivated_at, last_synced_at, raw
            )
            SELECT
                $1,
                a->>'id',
                a->>'displayName',
                a->>'mask',
                a->'type'->>'name',
                a->'subtype'->>'name',
                COALESCE(a->'credential'->'institution'->>'id', a->'institution'->>'id'),
                COALESCE(a->'credential'->'institution'->>'name', a->'institution'->>'name'),
                a->>'logoUrl',
                (a->>'currentBalance')::numeric,
                (a->>'displayBalance')::numeric,
                (a->>'signedBalance')::numeric,
                (a->>'limit')::numeric,
                (a->>'includeInNetWorth')::boolean,
                (a->>'isHidden')::boolean,
                (a->>'isAsset')::boolean,
                (a->>'isManual')::boolean,
                (a->>'isTaxable')::boolean,
                (a->>'syncDisabled')::boolean,
                a->>'dataProvider',
                a->'credential'->>'id',
                (a->'credential'->>'updateRequired')::boolean,
                (a->>'holdingsCount')::integer,
                (a->>'order')::integer,
                (a->>'deactivatedAt')::timestamptz,
                (a->>'displayLastUpdatedAt')::timestamptz,
                a
            FROM jsonb_array_elements($2::jsonb) a
            "#,
        )
        .bind(run_id)
        .bind(json_array(accounts)?)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Upsert Monarch's daily balance history for one account.
    /// Returns the number of days that were new or changed.
    pub async fn upsert_account_balances(
        &self,
        run_id: Uuid,
        account_id: &str,
        snapshots: &[Value],
    ) -> Result<u64> {
        let mut changed = 0;
        for batch in snapshots.chunks(WRITE_BATCH_SIZE) {
            let result = sqlx::query(
                r#"
                INSERT INTO account_daily_balances (account_id, balance_date, balance, run_id)
                SELECT DISTINCT ON ((s->>'date')::date)
                    $2, (s->>'date')::date, (s->>'signedBalance')::numeric, $1
                FROM jsonb_array_elements($3::jsonb) s
                WHERE s->>'date' IS NOT NULL
                ON CONFLICT (account_id, balance_date) DO UPDATE SET
                    balance = EXCLUDED.balance,
                    run_id = EXCLUDED.run_id,
                    updated_at = NOW()
                WHERE account_daily_balances.balance IS DISTINCT FROM EXCLUDED.balance
                "#,
            )
            .bind(run_id)
            .bind(account_id)
            .bind(json_array(batch)?)
            .execute(&self.pool)
            .await?;
            changed += result.rows_affected();
        }

        Ok(changed)
    }

    /// Validate the entire selection before sending any edits to Monarch.
    pub async fn contains_transactions(&self, ids: &[String]) -> Result<bool> {
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM transactions WHERE transaction_id = ANY($1) AND deleted_at IS NULL")
            .bind(ids).fetch_one(&self.pool).await?;
        Ok(count as usize == ids.len())
    }

    /// Store only fields confirmed by Monarch, preserving unrelated raw data.
    pub async fn mirror_transaction_edit(&self, value: &Value, group: Option<&str>) -> Result<()> {
        let id = value["id"].as_str().context("Missing transaction id")?;
        for key in ["merchant", "category"] {
            if !value[key].is_object() || !value[key]["id"].is_string() || !value[key]["name"].is_string() {
                anyhow::bail!("Monarch did not return canonical {} fields", key);
            }
        }
        let result = sqlx::query(r#"
            UPDATE transactions SET
                merchant_id = $2::jsonb->'merchant'->>'id',
                merchant_name = $2::jsonb->'merchant'->>'name',
                category_id = $2::jsonb->'category'->>'id',
                category_name = $2::jsonb->'category'->>'name',
                category_group_type = COALESCE($3, category_group_type),
                raw = COALESCE(raw, '{}'::jsonb) || jsonb_build_object('merchant', $2::jsonb->'merchant',
                    'category', COALESCE(raw->'category', '{}'::jsonb) || ($2::jsonb->'category') ||
                        CASE WHEN $3::text IS NULL THEN '{}'::jsonb ELSE jsonb_build_object('group',
                            COALESCE($2::jsonb->'category'->'group', raw->'category'->'group', '{}'::jsonb) || jsonb_build_object('type', $3::text)) END),
                last_seen_at = NOW(), changed_at = NOW()
            WHERE transaction_id = $1 AND deleted_at IS NULL
        "#).bind(id).bind(value).bind(group).execute(&self.pool).await?;
        if result.rows_affected() != 1 { anyhow::bail!("Transaction changed in Monarch but is missing from the mirror"); }
        Ok(())
    }

    /// Upsert transactions into the mirror, along with their tags and merchants
    pub async fn upsert_transactions(&self, run_id: Uuid, transactions: &[Value]) -> Result<()> {
        for batch in transactions.chunks(WRITE_BATCH_SIZE) {
            let batch_json = json_array(batch)?;
            let mut tx = self.pool.begin().await?;

            // DISTINCT ON guards against a transaction appearing twice in one
            // batch when Monarch's offset pagination shifts mid-fetch
            sqlx::query(
                r#"
                INSERT INTO transactions (
                    run_id, transaction_id, account_id, date, original_date, amount, pending,
                    notes, original_name, plaid_name, data_provider_description,
                    merchant_id, merchant_name, category_id, category_name, category_group_type,
                    is_reviewed, review_status, needs_review, reviewed_at,
                    is_split, has_splits, split_parent_id, is_recurring, is_manual,
                    hide_from_reports, hidden_by_account, goal_id, owner_user_id,
                    attachments_count, monarch_created_at, monarch_updated_at, raw
                )
                SELECT DISTINCT ON (t->>'id')
                    $1,
                    t->>'id',
                    t->'account'->>'id',
                    (t->>'date')::date,
                    (t->>'originalDate')::date,
                    (t->>'amount')::numeric,
                    COALESCE((t->>'pending')::boolean, false),
                    t->>'notes',
                    t->>'plaidName',
                    t->>'plaidName',
                    t->>'dataProviderDescription',
                    t->'merchant'->>'id',
                    t->'merchant'->>'name',
                    t->'category'->>'id',
                    t->'category'->>'name',
                    t->'category'->'group'->>'type',
                    (t->>'reviewedAt') IS NOT NULL,
                    t->>'reviewStatus',
                    COALESCE((t->>'needsReview')::boolean, false),
                    (t->>'reviewedAt')::timestamptz,
                    COALESCE((t->>'isSplitTransaction')::boolean, false),
                    COALESCE((t->>'hasSplitTransactions')::boolean, false),
                    t->'originalTransaction'->>'id',
                    COALESCE((t->>'isRecurring')::boolean, false),
                    COALESCE((t->>'isManual')::boolean, false),
                    COALESCE((t->>'hideFromReports')::boolean, false),
                    COALESCE((t->>'hiddenByAccount')::boolean, false),
                    t->'goal'->>'id',
                    t->'ownedByUser'->>'id',
                    COALESCE(jsonb_array_length(NULLIF(t->'attachments', 'null')), 0),
                    (t->>'createdAt')::timestamptz,
                    (t->>'updatedAt')::timestamptz,
                    t
                FROM jsonb_array_elements($2::jsonb) t
                ON CONFLICT (transaction_id) DO UPDATE SET
                    run_id = EXCLUDED.run_id,
                    account_id = EXCLUDED.account_id,
                    date = EXCLUDED.date,
                    original_date = EXCLUDED.original_date,
                    amount = EXCLUDED.amount,
                    pending = EXCLUDED.pending,
                    notes = EXCLUDED.notes,
                    original_name = EXCLUDED.original_name,
                    plaid_name = EXCLUDED.plaid_name,
                    data_provider_description = EXCLUDED.data_provider_description,
                    merchant_id = EXCLUDED.merchant_id,
                    merchant_name = EXCLUDED.merchant_name,
                    category_id = EXCLUDED.category_id,
                    category_name = EXCLUDED.category_name,
                    category_group_type = EXCLUDED.category_group_type,
                    is_reviewed = EXCLUDED.is_reviewed,
                    review_status = EXCLUDED.review_status,
                    needs_review = EXCLUDED.needs_review,
                    reviewed_at = EXCLUDED.reviewed_at,
                    is_split = EXCLUDED.is_split,
                    has_splits = EXCLUDED.has_splits,
                    split_parent_id = EXCLUDED.split_parent_id,
                    is_recurring = EXCLUDED.is_recurring,
                    is_manual = EXCLUDED.is_manual,
                    hide_from_reports = EXCLUDED.hide_from_reports,
                    hidden_by_account = EXCLUDED.hidden_by_account,
                    goal_id = EXCLUDED.goal_id,
                    owner_user_id = EXCLUDED.owner_user_id,
                    attachments_count = EXCLUDED.attachments_count,
                    monarch_created_at = EXCLUDED.monarch_created_at,
                    monarch_updated_at = EXCLUDED.monarch_updated_at,
                    changed_at = CASE
                        WHEN transactions.raw IS DISTINCT FROM EXCLUDED.raw
                          OR transactions.deleted_at IS NOT NULL
                        THEN NOW()
                        ELSE transactions.changed_at
                    END,
                    raw = EXCLUDED.raw,
                    last_seen_at = NOW(),
                    deleted_at = NULL
                "#,
            )
            .bind(run_id)
            .bind(&batch_json)
            .execute(&mut *tx)
            .await?;

            sqlx::query(
                r#"
                DELETE FROM transaction_tags
                WHERE transaction_id IN (
                    SELECT t->>'id' FROM jsonb_array_elements($1::jsonb) t
                )
                "#,
            )
            .bind(&batch_json)
            .execute(&mut *tx)
            .await?;

            sqlx::query(
                r#"
                INSERT INTO transaction_tags (transaction_id, tag_id)
                SELECT DISTINCT t->>'id', tag->>'id'
                FROM jsonb_array_elements($1::jsonb) t,
                     jsonb_array_elements(NULLIF(t->'tags', 'null')) tag
                ON CONFLICT (transaction_id, tag_id) DO NOTHING
                "#,
            )
            .bind(&batch_json)
            .execute(&mut *tx)
            .await?;

            sqlx::query(
                r#"
                INSERT INTO merchants (id, name, logo_url)
                SELECT DISTINCT ON (t->'merchant'->>'id')
                    t->'merchant'->>'id', t->'merchant'->>'name', t->'merchant'->>'logoUrl'
                FROM jsonb_array_elements($1::jsonb) t
                WHERE t->'merchant'->>'id' IS NOT NULL
                ON CONFLICT (id) DO UPDATE SET
                    name = EXCLUDED.name,
                    logo_url = EXCLUDED.logo_url,
                    updated_at = NOW()
                WHERE merchants.name IS DISTINCT FROM EXCLUDED.name
                   OR merchants.logo_url IS DISTINCT FROM EXCLUDED.logo_url
                "#,
            )
            .bind(&batch_json)
            .execute(&mut *tx)
            .await?;

            tx.commit().await?;
        }

        Ok(())
    }

    /// Flag transactions this run should have seen but did not, i.e. ones
    /// deleted in Monarch. `since` limits the check to the fetched date window;
    /// `None` means the whole history was fetched.
    /// Returns the number of transactions newly flagged.
    pub async fn mark_missing_transactions_deleted(
        &self,
        run_id: Uuid,
        since: Option<NaiveDate>,
    ) -> Result<u64> {
        let result = sqlx::query(
            r#"
            UPDATE transactions
            SET deleted_at = NOW(), changed_at = NOW()
            WHERE deleted_at IS NULL
              AND run_id IS DISTINCT FROM $1
              AND ($2::date IS NULL OR date >= $2)
            "#,
        )
        .bind(run_id)
        .bind(since)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected())
    }

    /// Remove merchants that no transaction refers to any more: ones merged
    /// away or deleted in Monarch. Only safe after a fetch of the whole
    /// history, since the merchants table is otherwise only ever added to.
    /// Returns the number of merchants removed.
    pub async fn prune_unused_merchants(&self) -> Result<u64> {
        let result = sqlx::query(
            r#"
            DELETE FROM merchants m
            WHERE NOT EXISTS (
                SELECT 1 FROM transactions t
                WHERE t.merchant_id = m.id AND t.deleted_at IS NULL
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected())
    }

    /// Replace the record of which statement credits refund which purchases
    pub async fn replace_credit_offsets(&self, offsets: &[crate::credits::Offset]) -> Result<()> {
        let rows: Vec<Value> = offsets
            .iter()
            .map(|o| {
                serde_json::json!({
                    "credit": o.credit_transaction_id,
                    "purchase": o.purchase_transaction_id,
                    "amount": o.amount,
                })
            })
            .collect();
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM credit_offsets").execute(&mut *tx).await?;
        sqlx::query(
            r#"
            INSERT INTO credit_offsets (credit_transaction_id, purchase_transaction_id, amount)
            SELECT DISTINCT ON (o->>'credit') o->>'credit', o->>'purchase', (o->>'amount')::numeric
            FROM jsonb_array_elements($1::jsonb) o
            "#,
        )
        .bind(json_array(&rows)?)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Replace the stored equity holdings with a capture's, and record what
    /// they come to today
    pub async fn replace_equity(&self, holdings: &crate::equity::Holdings) -> Result<()> {
        let captured_at = holdings.captured_at.unwrap_or_else(chrono::Utc::now);
        let mut tx = self.pool.begin().await?;
        for table in [
            "equity_issuers",
            "equity_grants",
            "equity_vesting_events",
            "equity_certificates",
            "equity_exercises",
            "equity_tenders",
        ] {
            sqlx::query(&format!("DELETE FROM {table}")).execute(&mut *tx).await?;
        }
        sqlx::query(
            r#"
            INSERT INTO equity_issuers (issuer_id, name, fmv, held_since, cash_cost, captured_at, raw)
            SELECT (i->>'issuer_id')::bigint, i->>'name', (i->>'fmv')::numeric, (i->>'held_since')::date,
                   (i->>'cash_cost')::numeric, $2, i->'raw'
            FROM jsonb_array_elements($1::jsonb) i
            "#,
        )
        .bind(json_array(&holdings.issuers)?)
        .bind(captured_at)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            r#"
            INSERT INTO equity_grants (grant_id, issuer_id, label, option_type, issue_date, expiration_date,
                quantity, vested, exercised, exercisable, exercise_price, status, is_canceled,
                vesting_start, vesting_end, vesting_schedule, granted_as, stock_type, cliff, vesting_terms,
                acceleration, early_exercise, iso_disqualified, form_83b_status, raw)
            SELECT (g->>'grant_id')::bigint, (g->>'issuer_id')::bigint, g->>'label', g->>'option_type',
                   (g->>'issue_date')::date, (g->>'expiration_date')::date,
                   (g->>'quantity')::bigint, (g->>'vested')::bigint, (g->>'exercised')::bigint,
                   (g->>'exercisable')::bigint, (g->>'exercise_price')::numeric, g->>'status',
                   (g->>'is_canceled')::boolean, (g->>'vesting_start')::date, (g->>'vesting_end')::date,
                   g->>'vesting_schedule', g->>'granted_as', g->>'stock_type', g->>'cliff', g->>'vesting_terms',
                   g->>'acceleration', (g->>'early_exercise')::boolean, (g->>'iso_disqualified')::boolean,
                   g->>'form_83b_status', g->'raw'
            FROM jsonb_array_elements($1::jsonb) g
            "#,
        )
        .bind(json_array(&holdings.grants)?)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            r#"
            INSERT INTO equity_vesting_events (grant_id, vest_date, quantity, cumulative, iso_quantity,
                nso_quantity, exercised, has_vested, kind)
            SELECT DISTINCT ON ((e->>'grant_id')::bigint, (e->>'vest_date')::date)
                   (e->>'grant_id')::bigint, (e->>'vest_date')::date, (e->>'quantity')::bigint,
                   (e->>'cumulative')::bigint, (e->>'iso_quantity')::bigint, (e->>'nso_quantity')::bigint,
                   (e->>'exercised')::bigint, (e->>'has_vested')::boolean, e->>'kind'
            FROM jsonb_array_elements($1::jsonb) e
            WHERE e->>'vest_date' IS NOT NULL
            "#,
        )
        .bind(json_array(&holdings.vesting_events)?)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            r#"
            INSERT INTO equity_certificates (certificate_id, issuer_id, label, share_class, issue_date,
                acquired_date, quantity, cost, status, is_canceled, exercise_type, exercised_from,
                fmv_at_acquisition, stock_type, is_certificated, raw)
            SELECT (c->>'certificate_id')::bigint, (c->>'issuer_id')::bigint, c->>'label', c->>'share_class',
                   (c->>'issue_date')::date, (c->>'acquired_date')::date, (c->>'quantity')::bigint,
                   (c->>'cost')::numeric, c->>'status', (c->>'is_canceled')::boolean, c->>'exercise_type',
                   c->>'exercised_from', (c->>'fmv_at_acquisition')::numeric, c->>'stock_type',
                   (c->>'is_certificated')::boolean, c->'raw'
            FROM jsonb_array_elements($1::jsonb) c
            "#,
        )
        .bind(json_array(&holdings.certificates)?)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            r#"
            INSERT INTO equity_exercises (certificate_id, issuer_id, grant_id, grant_label, certificate_label,
                option_type, exercised_at, quantity, exercise_price, cost, fmv, payment_method, status,
                taxes, early_exercise, raw)
            SELECT DISTINCT ON ((x->>'certificate_id')::bigint)
                   (x->>'certificate_id')::bigint, (x->>'issuer_id')::bigint, (x->>'grant_id')::bigint,
                   x->>'grant_label', x->>'certificate_label', x->>'option_type',
                   (x->>'exercised_at')::timestamptz, (x->>'quantity')::bigint, (x->>'exercise_price')::numeric,
                   (x->>'cost')::numeric, (x->>'fmv')::numeric, x->>'payment_method', x->>'status',
                   (x->>'taxes')::numeric, (x->>'early_exercise')::boolean, x->'raw'
            FROM jsonb_array_elements($1::jsonb) x
            "#,
        )
        .bind(json_array(&holdings.exercises)?)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            r#"
            INSERT INTO equity_tenders (tender_id, issuer_name, name, status, kind, start_date, end_date,
                settlement_date, price, shares_sold, proceeds, raw)
            SELECT (t->>'tender_id')::bigint, t->>'issuer_name', t->>'name', t->>'status', t->>'kind',
                   (t->>'start_date')::date, (t->>'end_date')::date, (t->>'settlement_date')::date,
                   (t->>'price')::numeric, (t->>'shares_sold')::bigint, (t->>'proceeds')::numeric, t->'raw'
            FROM jsonb_array_elements($1::jsonb) t
            "#,
        )
        .bind(json_array(&holdings.tenders)?)
        .execute(&mut *tx)
        .await?;
        // One row per company per day; a later capture the same day replaces it
        sqlx::query(
            r#"
            INSERT INTO equity_snapshots (issuer_id, snapshot_date, fmv, shares, options_vested,
                options_unvested, shares_value, vested_options_value, unvested_options_value,
                cost_to_exercise_vested)
            SELECT i.issuer_id, ($1::timestamptz)::date, i.fmv,
                   COALESCE(c.shares, 0), COALESCE(g.vested, 0), COALESCE(g.unvested, 0),
                   COALESCE(c.shares, 0) * COALESCE(i.fmv, 0),
                   COALESCE(g.vested_value, 0), COALESCE(g.unvested_value, 0), COALESCE(g.cost, 0)
            FROM equity_issuers i
            LEFT JOIN (
                SELECT issuer_id, SUM(quantity) AS shares
                FROM equity_certificates WHERE NOT is_canceled GROUP BY issuer_id
            ) c USING (issuer_id)
            LEFT JOIN (
                SELECT g.issuer_id,
                       SUM(g.exercisable) AS vested,
                       SUM(g.quantity - g.vested) AS unvested,
                       SUM(g.exercisable * GREATEST(COALESCE(i.fmv, 0) - g.exercise_price, 0)) AS vested_value,
                       SUM((g.quantity - g.vested) * GREATEST(COALESCE(i.fmv, 0) - g.exercise_price, 0)) AS unvested_value,
                       SUM(g.exercisable * g.exercise_price) AS cost
                FROM equity_grants g JOIN equity_issuers i USING (issuer_id)
                WHERE NOT g.is_canceled GROUP BY g.issuer_id
            ) g USING (issuer_id)
            ON CONFLICT (issuer_id, snapshot_date) DO UPDATE SET
                fmv = EXCLUDED.fmv, shares = EXCLUDED.shares, options_vested = EXCLUDED.options_vested,
                options_unvested = EXCLUDED.options_unvested, shares_value = EXCLUDED.shares_value,
                vested_options_value = EXCLUDED.vested_options_value,
                unvested_options_value = EXCLUDED.unvested_options_value,
                cost_to_exercise_vested = EXCLUDED.cost_to_exercise_vested
            "#,
        )
        .bind(captured_at)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Insert or update category groups and categories
    pub async fn upsert_categories(&self, groups: &[Value], categories: &[Value]) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO category_groups (id, name, group_type, group_order, raw, updated_at)
            SELECT g->>'id', g->>'name', g->>'type', (g->>'order')::integer, g, NOW()
            FROM jsonb_array_elements($1::jsonb) g
            ON CONFLICT (id) DO UPDATE SET
                name = EXCLUDED.name,
                group_type = EXCLUDED.group_type,
                group_order = EXCLUDED.group_order,
                raw = EXCLUDED.raw,
                updated_at = NOW()
            "#,
        )
        .bind(json_array(groups)?)
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO categories (
                id, name, icon, group_id, group_name, group_type, category_order,
                is_system_category, is_disabled, exclude_from_budget, raw, updated_at
            )
            SELECT
                c->>'id',
                c->>'name',
                c->>'icon',
                c->'group'->>'id',
                g.name,
                c->'group'->>'type',
                (c->>'order')::integer,
                COALESCE((c->>'isSystemCategory')::boolean, false),
                COALESCE((c->>'isDisabled')::boolean, false),
                COALESCE((c->>'excludeFromBudget')::boolean, false),
                c,
                NOW()
            FROM jsonb_array_elements($1::jsonb) c
            LEFT JOIN category_groups g ON g.id = c->'group'->>'id'
            ON CONFLICT (id) DO UPDATE SET
                name = EXCLUDED.name,
                icon = EXCLUDED.icon,
                group_id = EXCLUDED.group_id,
                group_name = EXCLUDED.group_name,
                group_type = EXCLUDED.group_type,
                category_order = EXCLUDED.category_order,
                is_system_category = EXCLUDED.is_system_category,
                is_disabled = EXCLUDED.is_disabled,
                exclude_from_budget = EXCLUDED.exclude_from_budget,
                raw = EXCLUDED.raw,
                updated_at = NOW()
            "#,
        )
        .bind(json_array(categories)?)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Insert or update tags
    pub async fn upsert_tags(&self, tags: &[Value]) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO tags (id, name, color, tag_order, updated_at)
            SELECT t->>'id', t->>'name', t->>'color', (t->>'order')::integer, NOW()
            FROM jsonb_array_elements($1::jsonb) t
            ON CONFLICT (id) DO UPDATE SET
                name = EXCLUDED.name,
                color = EXCLUDED.color,
                tag_order = EXCLUDED.tag_order,
                updated_at = NOW()
            "#,
        )
        .bind(json_array(tags)?)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Store this run's investment snapshot. `holdings` are Monarch's
    /// aggregate holdings (one per security), each carrying the per-account
    /// positions and tax lots that make it up.
    pub async fn insert_portfolio(
        &self,
        run_id: Uuid,
        holdings: &[Value],
        performance: &Value,
    ) -> Result<()> {
        let holdings_json = json_array(holdings)?;
        let mut tx = self.pool.begin().await?;

        sqlx::query(
            r#"
            INSERT INTO securities (
                id, ticker, name, security_type, security_type_display, asset_class,
                current_price, current_price_updated_at, closing_price, raw, updated_at
            )
            SELECT DISTINCT ON (s->>'id')
                s->>'id',
                s->>'ticker',
                s->>'name',
                s->>'type',
                s->>'typeDisplay',
                s->>'assetClass',
                (s->>'currentPrice')::numeric,
                (s->>'currentPriceUpdatedAt')::timestamptz,
                (s->>'closingPrice')::numeric,
                s,
                NOW()
            FROM jsonb_array_elements($1::jsonb) h,
                 LATERAL (SELECT h->'security' AS s) sec
            WHERE s->>'id' IS NOT NULL
            ON CONFLICT (id) DO UPDATE SET
                ticker = EXCLUDED.ticker,
                name = EXCLUDED.name,
                security_type = EXCLUDED.security_type,
                security_type_display = EXCLUDED.security_type_display,
                asset_class = EXCLUDED.asset_class,
                current_price = EXCLUDED.current_price,
                current_price_updated_at = EXCLUDED.current_price_updated_at,
                closing_price = EXCLUDED.closing_price,
                raw = EXCLUDED.raw,
                updated_at = NOW()
            "#,
        )
        .bind(&holdings_json)
        .execute(&mut *tx)
        .await?;

        // The nested positions are stored in their own table, so they are
        // dropped from the aggregate row's raw copy to avoid duplicating them
        sqlx::query(
            r#"
            INSERT INTO holdings (
                run_id, holding_id, security_id, security_name, security_type, ticker,
                quantity, price, value, cost_basis, gain_loss, gain_loss_percent,
                price_change_dollars, price_change_percent, is_cash, last_synced_at, raw
            )
            SELECT
                $1,
                h->>'id',
                h->'security'->>'id',
                h->'security'->>'name',
                h->'security'->>'type',
                h->'security'->>'ticker',
                (h->>'quantity')::numeric,
                (h->'security'->>'currentPrice')::numeric,
                (h->>'totalValue')::numeric,
                (h->>'costBasis')::numeric,
                (h->>'totalValue')::numeric - (h->>'costBasis')::numeric,
                CASE WHEN (h->>'costBasis')::numeric <> 0
                     THEN ((h->>'totalValue')::numeric - (h->>'costBasis')::numeric)
                          / (h->>'costBasis')::numeric * 100
                END,
                (h->>'securityPriceChangeDollars')::numeric,
                (h->>'securityPriceChangePercent')::numeric,
                COALESCE(h->'security'->>'type' = 'cash', false),
                (h->>'lastSyncedAt')::timestamptz,
                h - 'holdings'
            FROM jsonb_array_elements($2::jsonb) h
            "#,
        )
        .bind(run_id)
        .bind(&holdings_json)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO positions (
                run_id, position_id, aggregate_holding_id, account_id, security_id,
                ticker, name, security_type, security_type_display,
                quantity, closing_price, closing_price_updated_at, value,
                cost_basis, user_cost_basis, gain_loss, gain_loss_percent,
                tax_lots_count, raw
            )
            SELECT
                $1,
                p->>'id',
                h->>'id',
                p->'account'->>'id',
                h->'security'->>'id',
                COALESCE(p->>'ticker', h->'security'->>'ticker'),
                COALESCE(p->>'name', h->'security'->>'name'),
                COALESCE(p->>'type', h->'security'->>'type'),
                COALESCE(p->>'typeDisplay', h->'security'->>'typeDisplay'),
                (p->>'quantity')::numeric,
                (p->>'closingPrice')::numeric,
                (p->>'closingPriceUpdatedAt')::timestamptz,
                (p->>'value')::numeric,
                (p->>'costBasis')::numeric,
                (p->>'userCostBasis')::numeric,
                (p->>'value')::numeric - (p->>'costBasis')::numeric,
                CASE WHEN (p->>'costBasis')::numeric <> 0
                     THEN ((p->>'value')::numeric - (p->>'costBasis')::numeric)
                          / (p->>'costBasis')::numeric * 100
                END,
                COALESCE(jsonb_array_length(NULLIF(p->'taxLots', 'null')), 0),
                p
            FROM jsonb_array_elements($2::jsonb) h,
                 jsonb_array_elements(NULLIF(h->'holdings', 'null')) p
            "#,
        )
        .bind(run_id)
        .bind(&holdings_json)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO position_tax_lots (
                lot_id, position_id, account_id, security_id, acquisition_date,
                acquisition_quantity, cost_basis_per_unit, monarch_created_at, run_id
            )
            SELECT DISTINCT ON (l->>'id')
                l->>'id',
                p->>'id',
                p->'account'->>'id',
                h->'security'->>'id',
                (l->>'acquisitionDate')::date,
                (l->>'acquisitionQuantity')::numeric,
                (l->>'costBasisPerUnit')::numeric,
                (l->>'createdAt')::timestamptz,
                $1
            FROM jsonb_array_elements($2::jsonb) h,
                 jsonb_array_elements(NULLIF(h->'holdings', 'null')) p,
                 jsonb_array_elements(NULLIF(p->'taxLots', 'null')) l
            ON CONFLICT (lot_id) DO UPDATE SET
                position_id = EXCLUDED.position_id,
                account_id = EXCLUDED.account_id,
                security_id = EXCLUDED.security_id,
                acquisition_date = EXCLUDED.acquisition_date,
                acquisition_quantity = EXCLUDED.acquisition_quantity,
                cost_basis_per_unit = EXCLUDED.cost_basis_per_unit,
                monarch_created_at = EXCLUDED.monarch_created_at,
                run_id = EXCLUDED.run_id,
                last_seen_at = NOW()
            "#,
        )
        .bind(run_id)
        .bind(&holdings_json)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO portfolio_snapshots (
                run_id, total_value, total_change_dollars, total_change_percent,
                one_day_change_percent
            )
            SELECT
                $1,
                (p->>'totalValue')::numeric,
                (p->>'totalChangeDollars')::numeric,
                (p->>'totalChangePercent')::numeric,
                (p->>'oneDayChangePercent')::numeric
            FROM (SELECT $2::jsonb AS p) perf
            "#,
        )
        .bind(run_id)
        .bind(serde_json::to_string(performance)?)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(())
    }

    /// Insert recurring streams
    pub async fn insert_recurring_streams(
        &self,
        run_id: Uuid,
        streams: &[RecurringStream],
    ) -> Result<()> {
        for stream in streams {
            let merchant_id = stream.merchant.as_ref().map(|m| m.id.clone());
            let merchant_name = stream.merchant.as_ref().and_then(|m| m.name.clone());
            let logo_url = stream
                .logo_url
                .clone()
                .or_else(|| stream.merchant.as_ref().and_then(|m| m.logo_url.clone()));
            // Monarch no longer exposes a category or last-seen date per stream
            let category_id: Option<String> = None;
            let last_date: Option<NaiveDate> = None;

            let next_date = stream
                .next_forecasted_date
                .as_ref()
                .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok());
            let raw = stream
                .raw
                .as_ref()
                .map(serde_json::to_string)
                .transpose()?;

            sqlx::query(
                r#"
                INSERT INTO recurring_streams (
                    run_id, stream_id, frequency, stream_type, review_status, is_active,
                    is_approved, merchant_id, merchant_name, display_name, logo_url, amount,
                    next_expected_date, last_date, category_id, account_id, raw
                )
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17::jsonb)
                "#,
            )
            .bind(run_id)
            .bind(&stream.id)
            .bind(&stream.frequency)
            .bind(&stream.stream_type)
            .bind(&stream.review_status)
            .bind(stream.is_active.unwrap_or(true))
            .bind(stream.is_approved())
            .bind(&merchant_id)
            .bind(&merchant_name)
            .bind(&stream.display_name)
            .bind(&logo_url)
            .bind(stream.amount)
            .bind(next_date)
            .bind(last_date)
            .bind(&category_id)
            .bind(&stream.account_id)
            .bind(&raw)
            .execute(&self.pool)
            .await?;
        }

        Ok(())
    }

    /// Upsert daily net worth history
    pub async fn upsert_aggregate_snapshots(&self, run_id: Uuid, snapshots: &[Value]) -> Result<()> {
        for batch in snapshots.chunks(WRITE_BATCH_SIZE) {
            sqlx::query(
                r#"
                INSERT INTO aggregate_snapshots (
                    run_id, snapshot_date, assets_total, liabilities_total, net_worth
                )
                SELECT DISTINCT ON ((s->>'date')::date)
                    $1,
                    (s->>'date')::date,
                    (s->>'assetsBalance')::numeric,
                    (s->>'liabilitiesBalance')::numeric,
                    COALESCE(
                        (s->>'balance')::numeric,
                        (s->>'assetsBalance')::numeric - COALESCE((s->>'liabilitiesBalance')::numeric, 0)
                    )
                FROM jsonb_array_elements($2::jsonb) s
                WHERE s->>'date' IS NOT NULL
                ON CONFLICT (snapshot_date) DO UPDATE SET
                    run_id = EXCLUDED.run_id,
                    assets_total = EXCLUDED.assets_total,
                    liabilities_total = EXCLUDED.liabilities_total,
                    net_worth = EXCLUDED.net_worth,
                    updated_at = NOW()
                WHERE aggregate_snapshots.net_worth IS DISTINCT FROM EXCLUDED.net_worth
                   OR aggregate_snapshots.assets_total IS DISTINCT FROM EXCLUDED.assets_total
                   OR aggregate_snapshots.liabilities_total IS DISTINCT FROM EXCLUDED.liabilities_total
                "#,
            )
            .bind(run_id)
            .bind(json_array(batch)?)
            .execute(&self.pool)
            .await?;
        }

        Ok(())
    }

    /// Store a whole API response under `kind`, replacing today's version
    pub async fn upsert_document(&self, run_id: Uuid, kind: &str, payload: &Value) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO documents (kind, payload, run_id)
            VALUES ($1, $2::jsonb, $3)
            ON CONFLICT (kind, as_of_date) DO UPDATE SET
                payload = EXCLUDED.payload,
                run_id = EXCLUDED.run_id,
                updated_at = NOW()
            "#,
        )
        .bind(kind)
        .bind(serde_json::to_string(payload)?)
        .bind(run_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}

#[cfg(test)]
mod edit_tests {
    use super::*;
    #[tokio::test]
    async fn confirmed_edits_preserve_other_transaction_fields() {
        // Use a disposable database only; never point this test at a personal mirror.
        let Ok(url) = std::env::var("NESTEGG_TEST_DATABASE_URL") else { return; };
        let db = Database::connect(&url).await.unwrap();
        let original = serde_json::json!({"notes":"keep this", "amount":-5, "category":{"id":"old", "group":{"id":"old-group","type":"expense"}}, "tags":[{"id":"keep-tag"}]});
        sqlx::query("INSERT INTO transactions(transaction_id, account_id, date, amount, notes, category_id, category_group_type, raw) VALUES ('api-test', 'test-account', '2026-10-01', -5, 'keep this', 'old', 'expense', $1)")
            .bind(&original).execute(&db.pool).await.unwrap();
        let canonical = serde_json::json!({"id":"api-test", "merchant":{"id":"new-merchant","name":"New merchant"}, "category":{"id":"new-category","name":"Income","group":{"id":"new-group","type":"income"}}});
        db.mirror_transaction_edit(&canonical, Some("income")).await.unwrap();
        let row = sqlx::query("SELECT merchant_name, category_name, category_group_type, notes, amount::float8 AS amount, raw FROM transactions WHERE transaction_id='api-test'").fetch_one(&db.pool).await.unwrap();
        use sqlx::Row;
        assert_eq!(row.get::<String,_>("merchant_name"), "New merchant");
        assert_eq!(row.get::<String,_>("category_group_type"), "income");
        assert_eq!(row.get::<String,_>("category_name"), "Income");
        assert_eq!(row.get::<String,_>("notes"), "keep this");
        assert_eq!(row.get::<f64,_>("amount"), -5.0);
        let raw: Value = row.get("raw");
        assert_eq!(raw["tags"], original["tags"]);
        assert_eq!(raw["notes"], original["notes"]);
        assert_eq!(raw["category"]["group"]["id"], "new-group");
    }
}
