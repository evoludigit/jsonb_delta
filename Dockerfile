# Dockerfile for jsonb_delta PostgreSQL Extension
# Multi-stage build optimized for security scanning

ARG PG_MAJOR=18

FROM rust:1-slim-bookworm AS base

ARG PG_MAJOR

# Install build essentials and PostgreSQL dev tools. Debian ships only one
# PostgreSQL major per release, so pull PG_MAJOR from the PGDG repository.
RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential \
        ca-certificates \
        curl \
        libclang-dev \
        pkg-config \
    && install -d /usr/share/postgresql-common/pgdg \
    && curl -fsSL -o /usr/share/postgresql-common/pgdg/apt.postgresql.org.asc \
        https://www.postgresql.org/media/keys/ACCC4CF8.asc \
    && echo "deb [signed-by=/usr/share/postgresql-common/pgdg/apt.postgresql.org.asc] https://apt.postgresql.org/pub/repos/apt bookworm-pgdg main" \
        > /etc/apt/sources.list.d/pgdg.list \
    && apt-get update && apt-get install -y --no-install-recommends \
        postgresql-server-dev-${PG_MAJOR} \
    && rm -rf /var/lib/apt/lists/*

# Install pgrx
RUN cargo install --locked cargo-pgrx --version 0.17.0

FROM base AS builder

ARG PG_MAJOR

# Initialize pgrx (cached)
RUN cargo pgrx init --pg${PG_MAJOR}=/usr/lib/postgresql/${PG_MAJOR}/bin/pg_config

WORKDIR /build

# Copy source code
COPY Cargo.toml Cargo.lock ./
COPY src/ ./src/
COPY sql/ ./sql/
COPY jsonb_delta.control ./

# Build and package extension
RUN cargo pgrx package --pg-config=/usr/lib/postgresql/${PG_MAJOR}/bin/pg_config

# Install security updates
RUN apt-get update && \
    apt-get upgrade -y --no-install-recommends \
        libxml2 \
        libxslt1.1 \
        && rm -rf /var/lib/apt/lists/*

# Labels for security scanning
LABEL org.opencontainers.image.title="jsonb_delta"
LABEL org.opencontainers.image.description="Efficient JSONB delta and patch operations for PostgreSQL"
LABEL org.opencontainers.image.version="0.3.1"
LABEL org.opencontainers.image.vendor="Evolution Digitale"
LABEL org.opencontainers.image.licenses="PostgreSQL"
LABEL org.opencontainers.image.source="https://github.com/evoludigit/jsonb_delta"

# Simple health check
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD echo "Container ready for security scanning"
