#!/bin/bash
set -e

# nestegg container entrypoint
# Orchestrates the browser logins and the Rust CLI

cd /app

log() {
    echo "{\"timestamp\":\"$(date -Iseconds)\",\"level\":\"info\",\"event\":\"entrypoint\",\"message\":\"$1\"}"
}

error() {
    echo "{\"timestamp\":\"$(date -Iseconds)\",\"level\":\"error\",\"event\":\"entrypoint\",\"message\":\"$1\"}"
}

# Function to run browser refresh
run_browser_refresh() {
    log "Starting browser cookie refresh"
    cd /app/browser

    if npm run refresh; then
        log "Browser refresh completed successfully"
        return 0
    else
        error "Browser refresh failed"
        return 1
    fi
}

# Function to run Rust scraper
run_scraper() {
    log "Starting Rust scraper"

    # Session cookies exported by the browser refresh authenticate the CLI
    local args=()
    args+=("--cookies" "${COOKIES_PATH:-/app/output/cookies.txt}")

    if [ -f "${DEVICE_UUID_PATH:-/app/output/device-uuid.txt}" ]; then
        args+=("--device-uuid-file" "${DEVICE_UUID_PATH:-/app/output/device-uuid.txt}")
    fi

    if [ -n "$DATABASE_URL" ]; then
        args+=("--database" "$DATABASE_URL")
    fi

    if [ -n "$NTFY_URL" ]; then
        args+=("--ntfy-url" "$NTFY_URL")
    fi

    if [ -n "$TRANSACTION_DAYS" ]; then
        args+=("--transaction-days" "$TRANSACTION_DAYS")
    fi

    if [ -n "$HISTORY_DAYS" ]; then
        args+=("--history-days" "$HISTORY_DAYS")
    fi

    if [ "$FORCE_REFRESH" = "true" ]; then
        args+=("--force-refresh")
    fi

    if [ "$VERBOSE" = "true" ]; then
        args+=("--verbose")
    fi

    if monarch-scraper "${args[@]}"; then
        log "Scraper completed successfully"
        return 0
    else
        error "Scraper failed"
        return 1
    fi
}

# Refile card statement credits so each nets out against its purchase.
# Runs before the scrape so the mirror picks up the corrected transactions.
# A failure here is logged but does not stop the scrape.
run_credit_reconciliation() {
    if [ "$RECONCILE_CREDITS" != "true" ]; then
        return 0
    fi
    log "Reconciling statement credits"
    local args=("--cookies" "${COOKIES_PATH:-/app/output/cookies.txt}" "--reconcile-credits")
    if [ -f "${DEVICE_UUID_PATH:-/app/output/device-uuid.txt}" ]; then
        args+=("--device-uuid-file" "${DEVICE_UUID_PATH:-/app/output/device-uuid.txt}")
    fi
    if ! monarch-scraper "${args[@]}"; then
        error "Statement credit reconciliation failed"
    fi
}

# Capture equity holdings from Carta and load them into the database
run_carta() {
    log "Starting Carta capture"
    cd /app/browser
    if ! npm run carta; then
        error "Carta capture failed"
        return 1
    fi
    monarch-scraper --load-carta "${CARTA_BUNDLE_PATH:-/app/output/carta-bundle.json}"
}

# Main entrypoint logic
case "${1:-scrape}" in
    schedule)
        # Scrape now and then every SYNC_INTERVAL_HOURS, for running as a
        # long-lived service where nothing else does the scheduling
        interval="${SYNC_INTERVAL_HOURS:-4}"
        while true; do
            if run_browser_refresh; then
                run_credit_reconciliation
                run_scraper || true
            fi
            log "Next sync in ${interval}h"
            sleep "${interval}h"
        done
        ;;

    carta)
        run_carta
        ;;

    scrape)
        # Full flow: refresh cookies then scrape
        if run_browser_refresh; then
            run_credit_reconciliation
            run_scraper
        else
            exit 1
        fi
        ;;

    force-scrape)
        # Full flow, but first make Monarch re-sync every account
        FORCE_REFRESH=true
        if run_browser_refresh; then
            run_credit_reconciliation
            run_scraper
        else
            exit 1
        fi
        ;;

    refresh)
        # Only refresh cookies (for testing)
        run_browser_refresh
        ;;

    cli)
        # Only run Rust CLI (assumes cookies exist)
        shift
        monarch-scraper "$@"
        ;;

    login)
        # Interactive login (not for production use)
        log "Starting interactive login"
        cd /app/browser
        npm run login
        ;;

    shell)
        # Debug shell
        exec /bin/bash
        ;;

    *)
        # Pass through to Rust CLI
        monarch-scraper "$@"
        ;;
esac
