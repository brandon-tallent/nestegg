# nestegg

nestegg mirrors your [Monarch Money](https://www.monarch.com) data (accounts,
transactions, investments, budgets, recurring bills) and, optionally, your
private company equity from [Carta](https://carta.com) into a local PostgreSQL
database, then lets you browse it with a fast keyboard-driven terminal app.

```
 Monarch Money ──┐
                 ├──▶  sync (container)  ──▶  PostgreSQL  ──▶  nestegg (terminal app)
 Carta ──────────┘      logs in, scrapes       your data
```

- **Terminal app** (`nestegg`): net worth, accounts, transactions with search,
  cash flow, reports, budget, recurring payments, goals, investments, and an
  equity screen with option grants, ISO/NSO splits, vesting and tender sales.
  See [tui/README.md](tui/README.md).
- **Sync**: a container that logs into Monarch with your credentials and
  mirrors everything every few hours.

## A tour

Every screenshot here is the built-in demo household, which is made up and
needs no database. To look around it yourself before setting anything up:

```bash
cargo install --git https://github.com/brandon-tallent/nestegg nestegg --locked
nestegg --demo
```

Each screen is one key away (`1` to `9`, then `0`), `/` searches the one you
are on, and `?` lists every key.

**Dashboard.** Net worth over time, this month's spending against last
month's, the latest transactions, what is due next, investments, credit score
and goals.

![The Dashboard screen](docs/screenshots/dashboard.png)

**Accounts.** Every account, grouped by type. The selected one shows its
balance history, the payments expected through the end of the month and its
recent activity; a loan also shows when it will be paid off.

![The Accounts screen](docs/screenshots/accounts.png)

**Transactions.** The whole history, with pending, unreviewed and recurring
ones marked.

![The Transactions screen](docs/screenshots/transactions.png)

`/` narrows the list as you type. Words match merchants, categories, accounts
and dates in any order, and `>100` or `<50` test the amount:

![Transactions searched for groceries over $100](docs/screenshots/search.png)

**Cash Flow.** Income and spending month by month, and one month broken down
by category, group or merchant. Enter opens the transactions behind a row.

![The Cash Flow screen](docs/screenshots/cashflow.png)

**Reports.** Spending by category across recent months, each cell shaded by
its size within its row.

![The Reports screen](docs/screenshots/reports.png)

**Budget.** This month's plan from Monarch against what has been earned and
spent.

![The Budget screen](docs/screenshots/budget.png)

**Recurring.** Bills, subscriptions, transfers and paychecks, found from the
rhythm of your own history, with when each is next due.

![The Recurring screen](docs/screenshots/recurring.png)

**Investments.** Portfolio value, allocation, and holdings across accounts or
account by account. Goals, one key before it, tracks what you are saving
toward.

![The Investments screen](docs/screenshots/investments.png)

**Equity.** Private company holdings from Carta: option grants, shares held,
the vesting ahead, and what it all comes to at other share prices.

![The Equity screen](docs/screenshots/equity.png)

Enter opens a grant, a holding of shares, an exercise or a tender sale. A
grant shows its options at each stage, divided between ISO and NSO:

![An option grant opened on the Equity screen](docs/screenshots/equity-grant.png)

The screenshots are taken by [docs/screenshots/tour.tape](docs/screenshots/tour.tape).

## Quick start

You need Docker with Compose, and Rust (for the terminal app).

```bash
git clone https://github.com/brandon-tallent/nestegg && cd nestegg
cp .env.example .env
# edit .env: set POSTGRES_PASSWORD (openssl rand -hex 16), MONARCH_EMAIL, MONARCH_PASSWORD
docker compose up -d
```

This starts PostgreSQL (on `localhost:5432` only) and the sync service. To
build the image yourself instead of pulling the published one, set
`NESTEGG_IMAGE=nestegg:local` in `.env` and run `docker compose build`.

### First login

Monarch emails a 6-digit code the first time a new browser signs in. Watch
the sync log, and hand the code over when it asks:

```bash
docker compose logs -f sync            # wait for "Waiting for verification code"
docker compose exec sync monarch-otp 123456
```

The session is saved in a volume, so this is needed once. The first sync
pulls your full history and can take a few minutes; after that it runs every
4 hours (`SYNC_INTERVAL_HOURS`). To sync right now:

```bash
docker compose run --rm sync scrape
```

### Install the terminal app

```bash
cargo install --git https://github.com/brandon-tallent/nestegg nestegg --locked
export DATABASE_URL=postgresql://nestegg:YOUR_POSTGRES_PASSWORD@localhost:5432/nestegg
nestegg
```

`?` lists every key. `a` swaps names and amounts for stand-ins if you want to
show someone your screen, and `nestegg --demo` shows a made-up household
instead of yours.

## Carta equity (optional)

If you hold options or shares through Carta, set `CARTA_EMAIL` and
`CARTA_PASSWORD` in `.env`, then:

```bash
docker compose run --rm sync carta
# Carta texts a code; from a second terminal:
docker compose exec sync carta-otp 123456
```

This captures your holdings and loads them, which turns on the Equity screen
(`0`). Run it again whenever you want fresh numbers; it is not scheduled.

### Try the Equity screen with sample data

No Carta account needed; this loads a fictional company:

```bash
docker compose run --rm -v "$PWD/examples:/examples:ro" sync cli --load-carta /examples/carta-sample.json
nestegg --screen equity
```

## Day to day

```bash
docker compose logs -f sync                  # what the sync is doing
docker compose pull && docker compose up -d  # update
docker compose exec postgres pg_dump -U nestegg nestegg | gzip > nestegg-backup.sql.gz
```

The database schema is applied by the sync on every start, so updates need no
migration step.

## Settings

Everything is set in `.env`; see [.env.example](.env.example) for the full
list. The ones worth knowing:

| Variable | What it does |
|---|---|
| `SYNC_INTERVAL_HOURS` | Hours between syncs (default 4) |
| `RECONCILE_CREDITS` | `true` refiles card statement credits **in Monarch itself** so each nets out against its purchase. Off by default because it edits your data. |
| `NTFY_URL` | Push a notification after each sync. It includes your net worth, so use a private server or an unguessable topic. |
| `POSTGRES_PORT` | Host port for Postgres, bound to localhost only |

## Running on Kubernetes

`k8s/` has example manifests: a CronJob for the sync and PostgreSQL. See [docs/DEPLOYMENT.md](docs/DEPLOYMENT.md). With the stack in a
cluster, `nestegg` finds the database through `kubectl` on its own.

## Development

| Path | What | Build |
|---|---|---|
| `src/` | `monarch-scraper`, the Rust sync CLI (needs rustc 1.98+, cmake, clang) | `cargo build --release` |
| `tui/` | `nestegg`, the terminal app (its own crate) | `cargo build --manifest-path tui/Cargo.toml` |
| `browser/` | Playwright logins for Monarch and Carta | `cd browser && npm install` |
| `sql/schema.sql` | The database schema, embedded in the scraper | |

```bash
cargo test                                  # scraper
cargo test --manifest-path tui/Cargo.toml   # terminal app
```

[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) describes how the pieces fit and
the database schema.

## License

[MIT](LICENSE)
