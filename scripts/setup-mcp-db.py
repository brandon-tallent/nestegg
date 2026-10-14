#!/usr/bin/env python3
"""Run on the cluster node. Provision SELECT-only access; never print credentials."""
import base64
import json
import secrets
import subprocess
from urllib.parse import quote, urlsplit

NAMESPACE = "nestegg"
ROLE = "nestegg_mcp"
SECRET = "nestegg-mcp-secrets"


def kubectl(*args, **kwargs):
    return subprocess.run(["kubectl", "-n", NAMESPACE, *args], check=True,
                          text=True, stdout=subprocess.PIPE, **kwargs).stdout


def identifier(value):
    return '"' + value.replace('"', '""') + '"'


def main():
    existing = kubectl("get", "secret", SECRET, "--ignore-not-found", "-o", "json")
    config = json.loads(kubectl("get", "secret", "nestegg-secrets", "-o", "json"))["data"]
    admin_url = urlsplit(base64.b64decode(config["DATABASE_URL"]).decode())
    owner = base64.b64decode(config["POSTGRES_USER"]).decode()
    database = base64.b64decode(config["POSTGRES_DB"]).decode()
    create = ""
    if not existing:
        password = secrets.token_hex(32)
        # Fail on an unexpected pre-existing role, rather than rotating unknown credentials.
        create = f"CREATE ROLE {ROLE} LOGIN PASSWORD '{password}' NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION;"
        url = f"postgresql://{ROLE}:{quote(password)}@postgres.nestegg.svc.cluster.local:{admin_url.port or 5432}/{quote(database, safe='')}"
    else:
        url = base64.b64decode(json.loads(existing)["data"]["DATABASE_URL"]).decode()
        if urlsplit(url).username != ROLE:
            raise SystemExit("Existing MCP secret does not use nestegg_mcp; refusing to change it")
    sql = f"""BEGIN;
{create}
ALTER ROLE {ROLE} SET default_transaction_read_only = on;
GRANT CONNECT ON DATABASE {identifier(database)} TO {ROLE};
GRANT USAGE ON SCHEMA public TO {ROLE};
GRANT SELECT ON ALL TABLES IN SCHEMA public TO {ROLE};
ALTER DEFAULT PRIVILEGES FOR ROLE {identifier(owner)} IN SCHEMA public GRANT SELECT ON TABLES TO {ROLE};
COMMIT;
"""
    # SQL travels on stdin, never argv or a temporary plaintext file.
    kubectl("exec", "-i", "deploy/postgres", "--", "sh", "-c",
            'exec psql -v ON_ERROR_STOP=1 -U "$POSTGRES_USER" -d "$POSTGRES_DB"', input=sql)
    if not existing:
        manifest = {"apiVersion": "v1", "kind": "Secret",
                    "metadata": {"name": SECRET, "namespace": NAMESPACE},
                    "type": "Opaque", "stringData": {"DATABASE_URL": url}}
        kubectl("create", "-f", "-", input=json.dumps(manifest))
    print("Nestegg MCP read-only role and Kubernetes secret ready")


if __name__ == "__main__":
    main()
