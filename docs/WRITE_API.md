# Transaction write API

The terminal app reads PostgreSQL directly. Its optional merchant/category
edits use `nestegg-api`, keeping the Monarch session on the sync machine.
Only selected transactions change; renaming a merchant globally and creating
rules are separate CLI operations.

## Server

Build with the same toolchain as the scraper (Rust 1.98 or newer):

```sh
cargo build --release --locked --bin nestegg-api
```

Set these environment variables on the server:

- `DATABASE_URL`: the mirror database.
- `MONARCH_COOKIES_PATH`: the current sync's cookies file.
- `MONARCH_DEVICE_UUID_PATH`: the current sync's device UUID file.
- `NESTEGG_API_TOKEN`: a random bearer token of at least 32 characters.
- `NESTEGG_API_BIND`: default `127.0.0.1:30803`.

Run `nestegg-api` as a persistent service. It reads the session files anew for
each edit batch, so browser refreshes take effect without a restart. Keep the
listener private. For a Tailscale-only endpoint over an encrypted tailnet:

```sh
tailscale serve --bg --tcp=8083 tcp://127.0.0.1:30803
```

Store API credentials in your secret manager, outside Git. The container
image also includes the `nestegg-api` executable; override the entrypoint to
run it as a separate service if deploying in containers.

## Client

Set `NESTEGG_API_URL` and `NESTEGG_API_TOKEN` before launching the TUI. For
example, resolve them through a 1Password service account with access to the
credential item. These are separate from the TUI's `DATABASE_URL`.

On Transactions: `t` marks a row, `T` marks all visible rows, `u` clears the
marks. `m` edits merchant names; `C` picks a category. Both have a review step
with explicit Apply/Cancel. Anonymized mode cannot submit edits.

## Protocol

All endpoints require `Authorization: Bearer TOKEN`, including `GET /health`.
`POST /v1/transaction-edits` accepts:

```json
{"ids":["123456789"],"field":"merchant","value":"New merchant","dry_run":true}
```

`field` is `merchant` or `category`. Category values must match an existing
Monarch category. A dry run validates the selection and Monarch session
without mutating data. Batches contain 1–500 unique transaction IDs.

The server validates the entire batch before writes. It sends each edit to
Monarch, verifies the returned transaction ID, and stores the canonical
merchant/category reply in PostgreSQL while preserving unrelated fields.
Responses list `id`, `updated`, and `error` per transaction. Batches are not
atomic: a failure can leave earlier transactions updated. The TUI clears
only confirmed successes and keeps failures marked. It reloads its database
view after completion. Requests are never automatically retried after a
connection interruption; reload and inspect before trying again.
