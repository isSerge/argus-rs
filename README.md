# Argus EVM Monitor

Argus is a next-generation, open-source, self-hosted monitoring tool for EVM chains

## Project Setup

### Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) (latest stable version)
- [sqlx-cli](https://github.com/launchbadge/sqlx/tree/main/sqlx-cli) for database migrations.

### Installation

1.  **Clone the repository:**
    ```bash
    git clone https://github.com/isSerge/argus-rs
    cd argus-rs
    ```

2.  **Install `sqlx-cli`:**
    ```bash
    cargo install sqlx-cli
    ```

## Configuration

The application is configured via a `config.yaml` file in the project root.

Example `config.yaml` file:
```yaml
database_url: "sqlite://monitor.db"
rpc_urls:
  - "https://mainnet.infura.io/v3/YOUR_INFURA_PROJECT_ID"
  - "https://eth-mainnet.alchemyapi.io/v2/YOUR_ALCHEMY_API_KEY"
network_id: "mainnet"
# Optional: Configuration for the RPC retry policy.
# If this section is omitted, default values will be used.
retry_config:
  # The maximum number of retries for a request.
  max_retry: 10
  # The initial backoff delay in milliseconds.
  backoff_ms: 1000
  # The number of compute units per second to allow (for rate limiting).
  compute_units_per_second: 100

block_chunk_size: 5
polling_interval_ms: 10000
confirmation_blocks: 12
```

- `database_url`: The connection string for the SQLite database.
- `rpc_urls`: A list of RPC endpoint URLs for the EVM network. The application currently uses the first URL in the list.
- `network_id`: A unique identifier for the network being monitored (e.g., "mainnet", "sepolia").
- `retry_config`: Configuration for the RPC retry policy
- `block_chunk_size`: The size of the block chunk to process at once
- `polling_interval_ms`: The interval in milliseconds to poll for new blocks
- `confirmation_blocks`: Number of confirmation blocks to wait for before processing to prevent against small reorgs

## Logging

Argus uses the `tracing` crate for structured logging. You can control the verbosity of the logs using the `RUST_LOG` environment variable.

Examples:
- `RUST_LOG=info cargo run --release`: Only shows `INFO` level messages and above.
- `RUST_LOG=debug cargo run --release`: Shows `DEBUG` level messages and above.
- `RUST_LOG=argus=trace cargo run --release`: Shows `TRACE` level messages and above specifically for the `argus` crate.

For more detailed control, refer to the `tracing-subscriber` documentation on `EnvFilter`.

## Database Setup

The application uses `sqlx` to manage database migrations. The state is stored in a local SQLite database file, configured via the `database_url` in `config.yaml`.

The database file will be created automatically on the first run if it doesn't exist.

1.  **Ensure `database_url` is set** in your `config.yaml` (see Configuration section).

2.  **Run migrations:**
    This command will create the necessary tables in the database. Note that `sqlx-cli` typically reads the `DATABASE_URL` from an environment variable. You may need to set it explicitly for the command, e.g., `DATABASE_URL="sqlite:monitor.db" sqlx migrate run`.
    ```bash
    sqlx migrate run
    ```

## Running the Application

Once the setup is complete, you can run the application.

1.  **Build the project:**
    ```bash
    cargo build --release
    ```

2.  **Run the application:**
    ```bash
    cargo run --release
    ```
