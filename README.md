# nestegg

Your whole financial picture in a terminal, from a database you own.

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
show someone your screen.

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
