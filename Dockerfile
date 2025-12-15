# Dockerfile for jsonb_delta PostgreSQL Extension
# Multi-stage build for minimal production image

# =============================================================================
# Stage 1: Builder (Optimized)
# =============================================================================
# Use official Rust image with pre-installed tools
FROM rust:1.85-slim-bookworm AS builder

# Install only essential PostgreSQL build dependencies
RUN apt-get update && apt-get install -y \
    build-essential \
    libclang-dev \
    pkg-config \
    postgresql-server-dev-17 \
    && rm -rf /var/lib/apt/lists/*

# Install pgrx
RUN cargo install --locked cargo-pgrx --version 0.16.1

# Initialize pgrx with minimal setup
RUN cargo pgrx init --pg17=/usr/lib/postgresql/17/bin/pg_config

WORKDIR /build

# Copy source code
COPY Cargo.toml Cargo.lock ./
COPY src/ ./src/
COPY sql/ ./sql/
COPY jsonb_delta.control ./

# Build and package extension for PostgreSQL 17
RUN cargo pgrx package --pg-config=/usr/lib/postgresql/17/bin/pg_config

# =============================================================================
# Stage 2: Production
# =============================================================================
FROM postgres:17-bookworm

LABEL org.opencontainers.image.title="jsonb_delta"
LABEL org.opencontainers.image.description="Incremental JSONB View Maintenance for PostgreSQL"
LABEL org.opencontainers.image.version="0.1.0"
LABEL org.opencontainers.image.vendor="FraiseQL"
LABEL org.opencontainers.image.licenses="PostgreSQL"
LABEL org.opencontainers.image.source="https://github.com/evoludigit/jsonb_delta"

# Install security updates for known vulnerabilities
# CVE-2025-7425: libxslt heap use-after-free (requires both libxml2 and libxslt updates)
RUN apt-get update && \
    apt-get upgrade -y --no-install-recommends \
        libxml2 \
        libxslt1.1 \
        && rm -rf /var/lib/apt/lists/*

# Copy extension files from builder (pgrx package creates these paths)
COPY --from=builder /build/target/release/jsonb_delta-pg17/usr/share/postgresql/17/extension/* \
    /usr/share/postgresql/17/extension/
COPY --from=builder /build/target/release/jsonb_delta-pg17/usr/lib/postgresql/17/lib/* \
    /usr/lib/postgresql/17/lib/

# Create extension in template database (optional)
# RUN service postgresql start && \
#     psql -U postgres -c "CREATE EXTENSION jsonb_delta;" template1 && \
#     service postgresql stop

# Health check
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD pg_isready -U postgres || exit 1

# Expose PostgreSQL port
EXPOSE 5432

# Default command
CMD ["postgres"]
