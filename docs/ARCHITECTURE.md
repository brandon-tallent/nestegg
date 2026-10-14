# Architecture

nestegg has three parts: a sync that copies your data into PostgreSQL, the
database itself, and a terminal app that reads it.

```
 Monarch Money ──┐
                 ├──▶  sync  ──▶  PostgreSQL  ──▶  nestegg (terminal app)
 Carta ──────────┘
```

## Sync

The sync runs in one container image and has two halves.

**Browser login** (`browser/`, TypeScript with Playwright). Signs in to
Monarch with your credentials, handles the emailed verification code on a
first login, and saves the session so later runs need no code. It hands the
session to the scraper as a cookies file.

**Scraper** (`src/`, Rust, the `monarch-scraper` binary). Uses that session
to read Monarch's GraphQL API (accounts, transactions, holdings, recurring
payments, budgets, goals, balance history) and writes everything to
PostgreSQL in one run. It also carries a few maintenance commands for your
Monarch data, such as tagging transactions and reconciling statement credits;
`monarch-scraper --help` lists them.

Carta follows a simpler path. `browser/src/carta.ts` signs in and saves the
responses of the read-only requests Carta's own web app makes into a single
bundle file, and `monarch-scraper --load-carta` loads that file. It runs on
demand rather than on a schedule.

`docker-entrypoint.sh` ties these together: `scrape` is a login refresh
followed by a scrape, `schedule` repeats that every few hours, and `carta`
does the Carta capture and load. See [DEPLOYMENT.md](DEPLOYMENT.md) for the
full list of modes and settings.

## Database

The schema lives in `sql/schema.sql`. It is embedded in the scraper and
applied on every connect, so every statement in it is idempotent and there is
no separate migration step.

Broadly, tables come in four kinds:

- **Mirrors**: one row per Monarch object, updated in place (transactions,
  categories, merchants, securities, daily balances).
- **Snapshots**: one row per object per sync run, so history accumulates
  (accounts, holdings, positions).
- **Documents**: whole API responses kept as JSON, one version per day
  (budget, goals, credit score).
- **Equity**: Carta holdings, rebuilt from each capture (grants, vesting,
  certificates, exercises, tender sales).

Most tables also keep the original object in a `raw` JSON column, so a field
the schema does not break out yet is still there to query.

## Terminal app

`tui/` is a separate Rust crate (ratatui) that installs as `nestegg`. It is
read-only and talks only to PostgreSQL: it loads the mirror at startup, and
everything on screen, including recurring-payment detection and the equity
valuations, is computed locally from that. See [tui/README.md](../tui/README.md).

## MCP server

The `tui` crate's optional `mcp` feature builds `nestegg-mcp`, which serves
the mirror to AI assistants over MCP. It shares the terminal app's data,
forecast and equity modules, so its answers match the screens, and like the
app it only reads. See [MCP.md](MCP.md).
