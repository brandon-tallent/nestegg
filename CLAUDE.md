# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

nestegg pulls financial data into one PostgreSQL database and browses it in a terminal. The data comes from Monarch Money's GraphQL API (accounts, transactions, investments, budgets) and from Carta (private company equity). Everything is deployed as `nestegg`: the Compose project, the Kubernetes namespace, the database and its user, the `nestegg-secrets` secret and the container image. Only the Monarch-specific scraper keeps the name: the `monarch-scraper` binary and its `monarch-sync` CronJob. Components:

1. **Rust CLI** (`/src`) - API client that mirrors Monarch into PostgreSQL
2. **Browser Automation** (`/browser`) - Playwright-based login that keeps the session cookies fresh
3. **TUI** (`/tui`) - Standalone ratatui crate (`nestegg`) that reads the Postgres mirror; see `tui/README.md`. Not part of the image build. Check layouts with `nestegg --demo --dump --screen <name> --size 140x40`.

## Build Commands

```bash
# Rust CLI (wreq needs rustc 1.98+)
cargo build --release

# Browser automation
cd browser && npm install
```

## Running Locally

```bash
# Refresh session cookies via browser automation
# BROWSER_CHANNEL=chrome uses an installed Chrome instead of Playwright's bundled one.
# If /tmp is quota-limited, point TMPDIR at real disk or Chrome's pages crash.
cd browser && MONARCH_EMAIL=... MONARCH_PASSWORD=... npm run refresh

# Run the Rust scraper (applies sql/schema.sql itself)
./target/release/monarch-scraper \
  --cookies output/cookies.txt \
  --device-uuid-file output/device-uuid.txt \
  --database "postgresql://user:pass@localhost/monarch"

# Same, but first make Monarch re-sync every account with its institution
./target/release/monarch-scraper --cookies output/cookies.txt --force-refresh ...
```

## Architecture

### Data Flow
1. Browser automation (`browser/refresh.ts`) logs into Monarch and exports the session cookies and device UUID
2. Rust CLI sends the `session_id` cookie plus `x-csrftoken` header on GraphQL requests to `api.monarch.com/graphql`
3. Data is mirrored into PostgreSQL; every scrape creates a run record
4. The TUI reads the latest data straight from PostgreSQL

### Authentication
- Monarch's web app uses cookie auth. The old `Authorization: Token` header is gone; `--auth-token` is kept only as a legacy override.
- A new browser is challenged with a 6-digit code emailed to the account. `refresh.ts` waits on that page for the code: set `MONARCH_OTP`, or write it to `$OUTPUT_DIR/otp.txt` while it waits. Once `browser/state/monarch.json` exists the code is not asked for again.
- Monarch answers a GraphQL query containing any unknown field with an opaque HTTP 400, so add fields one at a time.

### Key Rust Modules
- `client.rs` - GraphQL API client built on `wreq`; returns objects both parsed and raw
- `db.rs` - PostgreSQL operations with `sqlx`; passes raw JSON batches to SQL, which extracts the columns
- `equity.rs` - Reads the Carta capture bundle into rows for the `equity_*` tables (`--load-carta BUNDLE`)
- `credits.rs` - Pairs card statement credits with the purchases they refund and plans the merchant/category changes that make them net out (`--reconcile-credits`, `--dry-run`); add a kind to `CREDIT_KINDS` for a new sort of credit
- `config.rs` - CLI argument parsing with `clap`, environment variable handling
- `models.rs` - GraphQL response types with `serde` deserialization

### Database Schema
Located in `/sql/schema.sql`, embedded in the binary and applied on every connect (keep every statement idempotent).
- **Mirrors** (one row per Monarch object, upserted): `transactions` (full history; `deleted_at` marks ones gone from Monarch), `transaction_tags`, `merchants`, `categories`, `category_groups`, `tags`, `securities`, `position_tax_lots`, `account_daily_balances`, `aggregate_snapshots`
- **Snapshots** (one row per object per run): `accounts`, `holdings` (per security), `positions` (per security per account), `portfolio_snapshots`, `recurring_streams`
- **Documents** (whole responses as JSONB, one version per day): `documents` with kinds `budget`, `goals`, `credit_score`, `household`, `household_members`
- Most tables have a `raw` JSONB column with the object exactly as Monarch returned it.
- **Equity** (rebuilt from each Carta capture): `equity_issuers`, `equity_grants`, `equity_vesting_events`, `equity_certificates`, `equity_exercises`, `equity_tenders`; `equity_snapshots` keeps one valuation per company per capture day
- Views: `latest_run`, `latest_accounts`, `latest_transactions`, `latest_holdings`, `latest_positions`, `latest_tax_lots`, `latest_recurring_streams`, `latest_documents`, `net_worth_history`.

## Carta Equity

Carta is refreshed by hand, since its login can ask for a texted code and
needs a full browser (run under xvfb):

```bash
cd browser && BROWSER_CHANNEL=chrome op run --env-file=carta.env -- npm run carta
./target/release/monarch-scraper --database "$DATABASE_URL" --load-carta output/carta-bundle.json
```

The capture only makes GET requests. When a code is needed it waits for
`output/carta-otp.txt`; `browser/state/carta.json` then keeps the session.
The bundle holds personal financial data and stays in the git-ignored `output/`.

## Environment Variables

**Scraper:**
- `DATABASE_URL` - PostgreSQL connection string
- `MONARCH_COOKIES_PATH` - Cookies file written by the browser refresh
- `MONARCH_FORCE_REFRESH`, `MONARCH_FORCE_REFRESH_TIMEOUT` - Re-sync accounts before scraping
- `MONARCH_DEVICE_UUID` or `MONARCH_DEVICE_UUID_PATH` - Request validation
- `NTFY_URL` - Notification webhook (optional)

**Browser:**
- `MONARCH_EMAIL`, `MONARCH_PASSWORD` - Login credentials
- `MONARCH_OTP` / `OTP_PATH` - Emailed verification code for a first login
- `BROWSER_CHANNEL` - e.g. `chrome` to drive an installed browser
- `OUTPUT_DIR` - Where to save cookies/state (default: `/app/output`)

## Deployment

Docker multi-stage build produces a single image for the sync. Entry point modes controlled via `docker-entrypoint.sh`:
- `scrape` - Refresh cookies then run CLI
- `force-scrape` - Same, after making Monarch re-sync every account
- `refresh` - Only update cookies
- `cli` - Run Rust CLI with custom args
- `schedule` - `scrape` now and every `SYNC_INTERVAL_HOURS`; what `docker-compose.yml` runs
- `carta` - Capture Carta holdings and load them
- `login` - Interactive browser login

`docker-compose.yml` is the single-machine deployment (Postgres and the sync on a schedule); `.env.example` lists its settings. `monarch-otp CODE` and `carta-otp CODE` inside the container hand a waiting login its verification code. `.github/workflows/ci.yml` tests both crates, smoke-tests the Compose stack with `examples/carta-sample.json`, and publishes the image to GHCR from `main`.

Kubernetes manifests in `/k8s/`: CronJob runs every 4 hours. Storage uses the cluster's default storage class. `monarch-sync-force` is a suspended CronJob for on-demand force refreshes:
`kubectl -n nestegg create job --from=cronjob/monarch-sync-force force-$(date +%s)`

## Code Patterns

- Financial amounts use `rust_decimal::Decimal` for precision
- All database operations reference a `run_id` for data versioning
- GraphQL queries are string templates in `queries.rs`

## Documentation

See `/docs` for detailed documentation:
- `ARCHITECTURE.md` - High-level overview of the sync, the database and the terminal app
- `DEPLOYMENT.md` - The image, entrypoint modes, Kubernetes manifests, troubleshooting
