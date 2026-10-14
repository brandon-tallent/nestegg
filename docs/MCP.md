# MCP

`nestegg-mcp` lets an AI assistant answer questions from the mirror. It
serves thirteen read-only tools over MCP's Streamable HTTP transport, built on
the terminal app's own code, so cash flow, recurring payments and equity values
match what the app shows. Nothing it does writes, and it has no SQL tool.

## Running it

Docker Compose starts it with the rest of the stack, at
`http://localhost:8091/mcp`. To reach it from other machines, put a proxy in
front of that port (`tailscale serve`, say) and add the name clients will use
to `MCP_ALLOWED_HOSTS` in `.env`.

On Kubernetes it has a slim image of its own (`Dockerfile.mcp`) and manifest
(`k8s/mcp.yaml`). `scripts/deploy-mcp.sh` builds it on the cluster node,
creates a SELECT-only database role and its secret, deploys it and exposes it
with Tailscale Serve. Its settings come from `k8s-local/mcp.env`, which git
ignores:

```bash
NODE=node1                      # ssh host of the cluster node
REGISTRY=registry.local:5000    # registry the node pushes to
MCP_HOST=node1.example.ts.net   # tailnet name, for the smoke test
MCP_ALLOWED_HOSTS=node1,node1.example.ts.net
```

## Connecting

```bash
claude mcp add --transport http nestegg http://localhost:8091/mcp
codex mcp add nestegg --url http://localhost:8091/mcp
```

`python3 scripts/smoke-mcp.py URL` calls every tool against a server with real
data and prints no figures.

## Tools

| Tool | Returns |
|---|---|
| `get_sync_status` | Recent runs and how current the data is; start here |
| `get_overview` | The dashboard: net worth, this month's cash flow, portfolio, credit score, equity |
| `get_accounts` | Balances, institutions, debt terms, sync health |
| `search_transactions` | Transactions by date, account, category, text or review state |
| `get_cash_flow` | Income, spending and savings by month and category |
| `get_history` | Net worth, account, investment or equity history |
| `get_document` | Budget, goals, credit score, household, members or transaction rules |
| `get_investments` | Holdings, positions per account, tax lots |
| `get_recurring` | Recurring payments and when they are next expected |
| `get_equity_summary` | Carta valuation and upcoming vesting |
| `get_equity` | Carta grants, certificates, exercises, tenders, vesting, snapshots |
| `get_record_detail` | One record in full, with the source's own JSON |
| `get_categories` | Categories, groups and tags |

Lists take `limit` and `offset`; follow `next_offset` until it is null.
Amounts are signed (money in is positive) and rounded to cents, except in
report spending, which is positive. Cash flow follows the app: no transfers,
hidden accounts or `One-time` items unless asked. Every result says when
Monarch and Carta were last synced.

There is no login. The server answers only the Host names it is given and
refuses browser requests, so keep it on localhost or a private network.
