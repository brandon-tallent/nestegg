#!/usr/bin/env bash
# Build on the cluster node, push to its registry, deploy only MCP, then
# expose it through Tailscale Serve. Settings come from the environment or
# k8s-local/mcp.env:
#   NODE               ssh host of the cluster node
#   REGISTRY           registry the node pushes to and the cluster pulls from
#   MCP_HOST           the node's tailnet name, for the smoke test
#   MCP_ALLOWED_HOSTS  extra Host names the server accepts (comma separated)
set -euo pipefail
cd "$(dirname "$0")/.."
if [[ -f k8s-local/mcp.env ]]; then
    set -a
    . k8s-local/mcp.env
    set +a
fi
: "${NODE:?set NODE, e.g. in k8s-local/mcp.env}" "${REGISTRY:?set REGISTRY}" "${MCP_HOST:?set MCP_HOST}"
MCP_ALLOWED_HOSTS="${MCP_ALLOWED_HOSTS:-$NODE,$MCP_HOST}"
TAG="${1:-$(date -u +%Y%m%dT%H%M%SZ)-$(git rev-parse --short HEAD)}"
# Relative to the remote user's home directory
REMOTE_DIR="${REMOTE_DIR:-build/nestegg-mcp}"
# Values are interpolated into remote shell commands; reject shell metacharacters.
for value in "$NODE" "$REGISTRY" "$MCP_HOST" "$MCP_ALLOWED_HOSTS" "$TAG" "$REMOTE_DIR"; do
    [[ "$value" =~ ^[a-zA-Z0-9_.:/,-]+$ ]] || { echo "Unsafe setting: $value" >&2; exit 2; }
done
IMAGE="$REGISTRY/nestegg/mcp:$TAG"
ssh "$NODE" "mkdir -p '$REMOTE_DIR' && sudo -n systemctl start docker"
COPYFILE_DISABLE=1 tar --no-xattrs -cf - Dockerfile.mcp Dockerfile.mcp.dockerignore tui/Cargo.toml tui/Cargo.lock tui/src \
    k8s/mcp.yaml scripts/setup-mcp-db.py scripts/smoke-mcp.py \
    | ssh "$NODE" "tar -xf - -C '$REMOTE_DIR'"
ssh "$NODE" "cd '$REMOTE_DIR' && docker build -f Dockerfile.mcp -t '$IMAGE' . && docker push '$IMAGE'"
ssh "$NODE" "python3 '$REMOTE_DIR/scripts/setup-mcp-db.py'"
# Render before apply so the Deployment never references the placeholder
# image, and add this node's names to the Host allowlist.
ssh "$NODE" "sed -e 's|image: nestegg-mcp:local|image: $IMAGE|' \
    -e 's|nestegg-mcp\.nestegg\.svc\.cluster\.local\$|&,$MCP_ALLOWED_HOSTS|' \
    '$REMOTE_DIR/k8s/mcp.yaml' | kubectl apply -f -"
ssh "$NODE" 'kubectl -n nestegg rollout status deploy/nestegg-mcp --timeout=180s'
# ClusterIP has no LAN NodePort. Persist only the tailnet listener; other Serve ports stay intact.
ssh "$NODE" 'ip=$(kubectl -n nestegg get svc nestegg-mcp -o jsonpath="{.spec.clusterIP}"); sudo -n tailscale serve --bg --tcp=8091 "tcp://$ip:8091"'
python3 scripts/smoke-mcp.py "http://$MCP_HOST:8091/mcp"
printf 'Deployed %s\nMCP URL: http://%s:8091/mcp\n' "$IMAGE" "$MCP_HOST"
