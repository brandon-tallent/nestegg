# Multi-stage Dockerfile for the nestegg backend: scraper and browser logins
# Stage 1: Build Rust CLI
# Stage 2: Build Node.js browser automation
# Stage 3: Final runtime image

# =============================================================================
# Stage 1: Build Rust CLI
# =============================================================================
FROM rust:latest AS rust-builder

# Install build dependencies for boring-sys2 (BoringSSL)
RUN apt-get update && apt-get install -y --no-install-recommends \
    cmake \
    clang \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Copy Cargo files for dependency caching
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release --locked && rm -rf src

# Copy actual source and build (the schema is embedded in the binary)
COPY sql ./sql
COPY src ./src
RUN touch src/main.rs && cargo build --release --locked

# =============================================================================
# Stage 2: Build Node.js browser automation
# =============================================================================
FROM node:22-bookworm AS node-builder

WORKDIR /build/browser

# Copy package files and install dependencies
COPY browser/package.json browser/package-lock.json browser/tsconfig.json ./
RUN npm ci

# Copy source files
COPY browser/src ./src

# =============================================================================
# Stage 3: Final runtime image
# =============================================================================
# Must match the playwright version pinned in browser/package.json, since the
# image ships the browser build that version drives
FROM mcr.microsoft.com/playwright:v1.64.0-noble

# ca-certificates is already installed in the Playwright base image
# Skipping apt-get to avoid clock skew issues in build environments

WORKDIR /app

# Copy Rust binary
COPY --from=rust-builder /build/target/release/monarch-scraper /build/target/release/nestegg-api /usr/local/bin/

# Copy browser automation
COPY --from=node-builder /build/browser ./browser
COPY --from=node-builder /build/browser/node_modules ./browser/node_modules

# Copy entrypoint script and the helpers that pass login codes to it
COPY docker-entrypoint.sh scripts/monarch-otp scripts/carta-otp /usr/local/bin/
RUN chmod +x /usr/local/bin/docker-entrypoint.sh /usr/local/bin/monarch-otp /usr/local/bin/carta-otp

# Create output directory for cookies and state
RUN mkdir -p /app/output /app/browser/state && \
    chown -R pwuser:pwuser /app

# Switch to non-root user (from Playwright base image)
USER pwuser

# Default environment variables
ENV NODE_ENV=production \
    OUTPUT_DIR=/app/output \
    COOKIES_PATH=/app/output/cookies.txt \
    DEVICE_UUID_PATH=/app/output/device-uuid.txt

LABEL org.opencontainers.image.source="https://github.com/brandon-tallent/nestegg" \
      org.opencontainers.image.description="nestegg backend: Monarch and Carta scrapers" \
      org.opencontainers.image.licenses="MIT"

ENTRYPOINT ["docker-entrypoint.sh"]
CMD ["scrape"]
