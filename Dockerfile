# ---- Chef ----
# Use the nightly rust image to match the project's toolchain
FROM rustlang/rust:nightly AS chef
# Install system dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential \
    cmake \
    pkg-config \
    libssl-dev \
    libsasl2-dev \
    && rm -rf /var/lib/apt/lists/*
# Install all the components specified in rust-toolchain.toml
RUN rustup component add rustc cargo rustfmt clippy rust-docs llvm-tools rust-src
WORKDIR /app
# Install cargo-chef
RUN cargo install cargo-chef

# ---- Planner ----
FROM chef AS planner
WORKDIR /app
COPY . .
# Compute a lock-like file for dependencies
RUN cargo chef prepare --recipe-path recipe.json

# ---- Builder ----
FROM chef AS builder
WORKDIR /app
COPY --from=planner /app/recipe.json recipe.json
# Build dependencies - this is the caching Docker layer
RUN cargo chef cook --release --recipe-path recipe.json

# ---- Application Builder ----
FROM builder AS app_builder
WORKDIR /app
COPY . .
# Build the application binary.
ENV SQLX_OFFLINE=true
RUN cargo build --release --bin argus

# ---- Final Image ----
FROM debian:bookworm-slim
RUN groupadd --system --gid 1001 appuser && \
    useradd --system --uid 1001 --gid 1001 appuser
RUN apt-get update && apt-get install -y --no-install-recommends \
    libssl3 \
    ca-certificates \
    librdkafka1 \
    libsasl2-modules \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
# Copy the application binary
COPY --from=app_builder /app/target/release/argus /usr/local/bin/
# Copy the migrations directory needed by sqlx at runtime.
COPY migrations /app/migrations
# Use a non-root user to run the application
RUN chown -R appuser:appuser /app
USER appuser
# Set the entrypoint
ENTRYPOINT ["/usr/local/bin/argus"]
