# Deployment

nestegg's backend is one container image (the Rust scraper, the Playwright
logins and the MCP server) plus PostgreSQL. The terminal app is installed
separately and only needs to reach the database.

For a single machine, use Docker Compose as described in the
[README](../README.md). This document covers the image itself and Kubernetes.

## The image

`ghcr.io/brandon-tallent/nestegg` is built from the `Dockerfile` by GitHub
Actions on every push to `main`. It has four stages: the Rust CLI (needs cmake
and clang for BoringSSL), the MCP server, the browser automation, and a runtime
based on Microsoft's Playwright image, which carries the browser. The
Playwright image tag must match the `playwright` version pinned in
`browser/package.json`.

```bash
docker build -t nestegg:local .
```

### Entrypoint modes

The first argument picks what the container does:

| Mode | What it does |
|---|---|
| `scrape` (default) | Refresh the Monarch session, then mirror everything once |
| `schedule` | `scrape` now and then every `SYNC_INTERVAL_HOURS`; what Compose runs |
| `force-scrape` | Make Monarch re-sync every account with its institution first |
| `refresh` | Only refresh the session cookies |
| `carta` | Capture Carta holdings and load them |
| `mcp` | Serve the [MCP server](MCP.md) on port 8091 |
| `cli ARGS…` | Run `monarch-scraper` with the given arguments |
| `shell` | A shell, for debugging |

### Environment

| Variable | Used by | Notes |
|---|---|---|
| `DATABASE_URL` | all | `postgresql://user:password@host:5432/dbname` |
| `MONARCH_EMAIL`, `MONARCH_PASSWORD` | sync | Monarch login |
| `MONARCH_OTP` | sync | Emailed code for a first login, if you would rather pass it up front than use `monarch-otp` |
| `SYNC_INTERVAL_HOURS` | `schedule` | Default 4 |
| `RECONCILE_CREDITS` | sync | `true` edits statement credits in Monarch |
| `TRANSACTION_DAYS`, `HISTORY_DAYS` | sync | Limit how far back to read; 0 or unset means everything |
| `NTFY_URL` | sync | Notification after each run |
| `CARTA_EMAIL`, `CARTA_PASSWORD`, `CARTA_MFA_METHOD`, `CARTA_OTP` | `carta` | Carta login |
| `NESTEGG_MCP_ALLOWED_HOSTS` | `mcp` | Host names clients may use; default `localhost,127.0.0.1,::1` |

### State

| Path | Holds |
|---|---|
| `/app/output` | Session cookies, device id, login codes in transit, the Carta capture |
| `/app/browser/state` | Saved browser sessions, so logins survive restarts |

Both hold live sessions: treat them like passwords. The container runs as
the Playwright image's unprivileged `pwuser`; the Kubernetes manifests run it
as uid 1000 with a matching `fsGroup`.

## Kubernetes

The manifests in `k8s/` are a working example for a small cluster. They use
the cluster's default storage class and the `nestegg` namespace throughout.

```bash
kubectl apply -f k8s/namespace.yaml

PASSWORD=$(openssl rand -hex 16)
kubectl -n nestegg create secret generic nestegg-secrets \
  --from-literal=POSTGRES_USER=nestegg \
  --from-literal=POSTGRES_DB=nestegg \
  --from-literal=POSTGRES_PASSWORD="$PASSWORD" \
  --from-literal=DATABASE_URL="postgresql://nestegg:$PASSWORD@postgres.nestegg.svc.cluster.local:5432/nestegg" \
  --from-literal=MONARCH_EMAIL=you@example.com \
  --from-literal=MONARCH_PASSWORD=...

kubectl apply -k k8s
```

`k8s/secrets.yaml` is a template showing the keys; do not commit real values.
To change the image, storage class or settings for your cluster without
editing the examples, put a kustomize overlay in `k8s-local/` (ignored by
git) with `../k8s` as its resource.

| Manifest | What |
|---|---|
| `cronjob.yaml` | `monarch-sync`: runs `scrape` every 4 hours |
| `cronjob-force.yaml` | `monarch-sync-force`: suspended; trigger by hand for a forced re-sync |
| `postgres.yaml` | PostgreSQL 16 with a 5Gi volume |
| `pvc.yaml` | `scraper-state`: the sync's cookies and browser sessions |

### First login

The first run waits for Monarch's emailed code. Start a job, then hand the
code to its pod:

```bash
kubectl -n nestegg create job --from=cronjob/monarch-sync first-sync
kubectl -n nestegg logs -f job/first-sync          # wait for "Waiting for verification code"
kubectl -n nestegg exec job/first-sync -- monarch-otp 123456
```

### Routine operations

```bash
# Sync now
kubectl -n nestegg create job --from=cronjob/monarch-sync manual-$(date +%s)

# Make Monarch re-sync every account, then sync
kubectl -n nestegg create job --from=cronjob/monarch-sync-force force-$(date +%s)

# Logs of the latest run
kubectl -n nestegg logs -l app=monarch-sync --tail=100

# Back up
kubectl -n nestegg exec deploy/postgres -- pg_dump -U nestegg nestegg | gzip > nestegg-backup.sql.gz
```

### The terminal app against a cluster

With no `--database` or `DATABASE_URL`, `nestegg` reads `DATABASE_URL` from
the `nestegg-secrets` secret and points it at the `postgres` service's
cluster IP, which works from a cluster node. `--namespace` (or
`NESTEGG_NAMESPACE`) picks a different namespace; the secret is expected to
be named `<namespace>-secrets`. From elsewhere, port-forward instead:

```bash
kubectl -n nestegg port-forward svc/postgres 5432:5432 &
nestegg --database postgresql://nestegg:PASSWORD@localhost:5432/nestegg
```

### The MCP server

`k8s/mcp.yaml` is applied on its own, by `scripts/deploy-mcp.sh`; see
[MCP.md](MCP.md).

## Troubleshooting

**The sync says it is waiting for a verification code.** Monarch is
challenging a new browser. Pass the emailed code with `monarch-otp` as above.
It waits 10 minutes, then gives up until the next run.

**Login fails after working for weeks.** The saved session expired or the
password changed. Check the credentials; the next run logs in again and may
ask for a new code. Screenshots of the page it was stuck on are written to
`/app/output`.

**Requests to Monarch start failing after working.** Update to the latest
image; Monarch changes its web app from time to time and the sync follows.

**A GraphQL query returns HTTP 400.** Monarch changed its schema: it answers
any query naming an unknown field this way. Run with `VERBOSE=true` to see
which query, and update it in `src/queries.rs`.

**The terminal app shows nothing.** Check that a sync has finished
(`docker compose logs sync`) and that `DATABASE_URL` points at the right
port and password.
