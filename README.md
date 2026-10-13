# nestegg

nestegg syncs [Monarch Money](https://www.monarch.com) & [Carta](https://carta.com)
data into a local postgres database and gives you a [terminal app](tui/README.md)
for looking through it.

<p align="center">
  <a href="docs/screenshots/dashboard.png"><img src="docs/screenshots/dashboard.png" width="32%" alt="Dashboard"></a>
  <a href="docs/screenshots/accounts.png"><img src="docs/screenshots/accounts.png" width="32%" alt="Accounts"></a>
  <a href="docs/screenshots/transactions.png"><img src="docs/screenshots/transactions.png" width="32%" alt="Transactions"></a>
  <a href="docs/screenshots/search.png"><img src="docs/screenshots/search.png" width="32%" alt="Searching transactions"></a>
  <a href="docs/screenshots/cashflow.png"><img src="docs/screenshots/cashflow.png" width="32%" alt="Cash Flow"></a>
  <a href="docs/screenshots/reports.png"><img src="docs/screenshots/reports.png" width="32%" alt="Reports"></a>
  <a href="docs/screenshots/budget.png"><img src="docs/screenshots/budget.png" width="32%" alt="Budget"></a>
  <a href="docs/screenshots/recurring.png"><img src="docs/screenshots/recurring.png" width="32%" alt="Recurring"></a>
  <a href="docs/screenshots/goals.png"><img src="docs/screenshots/goals.png" width="32%" alt="Goals"></a>
  <a href="docs/screenshots/investments.png"><img src="docs/screenshots/investments.png" width="32%" alt="Investments"></a>
  <a href="docs/screenshots/equity.png"><img src="docs/screenshots/equity.png" width="32%" alt="Equity"></a>
  <a href="docs/screenshots/equity-grant.png"><img src="docs/screenshots/equity-grant.png" width="32%" alt="An option grant"></a>
</p>

## Setup

```bash
git clone https://github.com/brandon-tallent/nestegg && cd nestegg
cp .env.example .env
# in .env, set POSTGRES_PASSWORD, MONARCH_EMAIL and MONARCH_PASSWORD
docker compose up -d
```

This starts Postgres, reachable only from localhost, and a sync container that
logs into Monarch and copies your data every 4 hours.

The first time it signs in, Monarch emails you a 6-digit code. Watch the log
and pass the code along when it asks:

```bash
docker compose logs -f sync            # wait for "Waiting for verification code"
docker compose exec sync monarch-otp 123456
```

You only do this once. The first sync fetches your whole history, which takes a
few minutes. Then install the app and point it at the database:

```bash
cargo install --git https://github.com/brandon-tallent/nestegg nestegg --locked
export DATABASE_URL=postgresql://nestegg:YOUR_POSTGRES_PASSWORD@localhost:5432/nestegg
nestegg
```

## Carta

If you have options or shares on Carta, add `CARTA_EMAIL` and `CARTA_PASSWORD`
to `.env` and run:

```bash
docker compose run --rm sync carta
# Carta texts you a code. From a second terminal:
docker compose exec sync carta-otp 123456
```

That fills in the Equity screen (`0`). It isn't on a schedule, so run it again
when you want fresh numbers.

## Settings

Settings live in `.env`, and [.env.example](.env.example) lists them all. A few
to know about:

- `SYNC_INTERVAL_HOURS` is how often to sync. The default is 4.
- `RECONCILE_CREDITS=true` refiles card statement credits so each one cancels
  out the purchase it refunds. This changes your data in Monarch itself, so it
  is off by default.
- `NTFY_URL` sends a notification after each sync. The message includes your
  net worth, so use a private server or a topic nobody could guess.
- `POSTGRES_PORT` is the port Postgres uses on localhost.
- Set `NESTEGG_IMAGE=nestegg:local` and run `docker compose build` to build the
  image yourself instead of pulling the published one.

## More

- [docs/DEPLOYMENT.md](docs/DEPLOYMENT.md) is about running on Kubernetes.
  `k8s/` has example manifests, and `nestegg` finds the database through
  `kubectl` by itself.
- [docs/WRITE_API.md](docs/WRITE_API.md) is about changing a transaction's
  merchant or category from the terminal app.
- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) explains how the pieces fit
  together and describes the database schema.

## Development

```bash
cargo build --release                        # src/: the sync CLI (needs Rust 1.98+, cmake, clang)
cargo build --manifest-path tui/Cargo.toml   # tui/: the terminal app, its own crate
cd browser && npm install                    # browser/: the Playwright logins

cargo test                                   # sync
cargo test --manifest-path tui/Cargo.toml    # terminal app
```

The database schema is `sql/schema.sql`. To test the Carta loader without a
Carta account, load the made-up capture in `examples/`:

```bash
docker compose run --rm -v "$PWD/examples:/examples:ro" sync cli --load-carta /examples/carta-sample.json
```

## License

[MIT](LICENSE)
