# Dockerfile for jsonb_delta PostgreSQL Extension
# Multi-stage build optimized for security scanning

FROM rust:1.85-slim-bookworm AS base

# Install build essentials and PostgreSQL dev tools
RUN apt-get update && apt-get install -y \
    build-essential \
    pkg-config \
    postgresql-server-dev-17 \
    && rm -rf /var/lib/apt/lists/*

# Install pgrx
RUN cargo install --locked cargo-pgrx --version 0.16.1

FROM base AS builder

# Initialize pgrx (cached)
RUN cargo pgrx init --pg17=/usr/lib/postgresql/17/bin/pg_config

WORKDIR /build

# Copy source code
COPY Cargo.toml Cargo.lock ./
COPY src/ ./src/
COPY sql/ ./sql/
COPY jsonb_delta.control ./

# Build and package extension
RUN cargo pgrx package --pg-config=/usr/lib/postgresql/17/bin/pg_config

# Install security updates
RUN apt-get update && \
    apt-get upgrade -y --no-install-recommends \
        libxml2 \
        libxslt1.1 \
        && rm -rf /var/lib/apt/lists/*

# Labels for security scanning
LABEL org.opencontainers.image.title="jsonb_delta"
LABEL org.opencontainers.image.description="Efficient JSONB delta and patch operations for PostgreSQL"
LABEL org.opencontainers.image.version="0.1.0"
LABEL org.opencontainers.image.vendor="Evolution Digitale"
LABEL org.opencontainers.image.licenses="PostgreSQL"
LABEL org.opencontainers.image.source="https://github.com/evoludigit/jsonb_delta"

# Simple health check
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD echo "Container ready for security scanning"
