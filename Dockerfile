# ---- Chef ----
# Use the nightly rust image to match the project's toolchain
FROM rustlang/rust:nightly AS chef
# Install all the components specified in rust-toolchain.toml to avoid
# cross-device link errors when rustup tries to install them later.
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
# Build the main application binary
RUN cargo build --release --bin argus
# Build the js_executor binary separately (not in workspace)
RUN cargo build --release --manifest-path crates/js_executor/Cargo.toml

# ---- Final Image ----
# Use a slim Debian Bookworm image to match the build environment and ensure
# compatibility with shared libraries like libssl.
FROM debian:bookworm-slim
# Install required shared libraries and certificates for HTTPS functionality.
RUN apt-get update && apt-get install -y libssl3 ca-certificates && rm -rf /var/lib/apt/lists/*
WORKDIR /app
# Copy the main application binary
COPY --from=app_builder /app/target/release/argus /usr/local/bin/
# Copy the js_executor binary  
COPY --from=app_builder /app/crates/js_executor/target/release/js_executor /usr/local/bin/
# Copy the migrations directory needed by sqlx at runtime.
COPY migrations /app/migrations
# Set the entrypoint
ENTRYPOINT ["/usr/local/bin/argus"]
