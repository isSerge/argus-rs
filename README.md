# Argus EVM Monitor

> **Note:** This project is under active development and is not yet ready for production use. The API and architecture are subject to change.

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
database_url: "sqlite:monitor.db"
rpc_urls:
  - "https://eth.llamarpc.com"
  - "https://1rpc.io/eth"
  - "https://rpc.mevblocker.io"
network_id: "mainnet"
block_chunk_size: 5
polling_interval_ms: 10000
confirmation_blocks: 12

# Optional: Configuration for the RPC retry policy.
# If this section is omitted, default values will be used.
retry_config:
  # The maximum number of retries for a request.
  max_retry: 10
  # The initial backoff delay in milliseconds.
  backoff_ms: 1000
  # The number of compute units per second to allow (for rate limiting).
  compute_units_per_second: 100

# Optional: Configuration for graceful shutdown timeout.
# If this section is omitted, the default value of 30 seconds will be used.
shutdown_timeout_secs: 30

# Optional: Security configuration for Rhai script execution.
# If this section is omitted, default security values will be used.
rhai:
  # Maximum number of operations a script can perform (default: 100000)
  max_operations: 100000
  # Maximum function call nesting depth (default: 10)
  max_call_levels: 10
  # Maximum size of strings in characters (default: 8192)
  max_string_size: 8192
  # Maximum number of array elements (default: 1000)
  max_array_size: 1000
  # Maximum execution time per script in milliseconds (default: 5000)
  execution_timeout: 5000
```

### Configuration Parameters

- `database_url`: The connection string for the SQLite database.
- `rpc_urls`: A list of RPC endpoint URLs for the EVM network. The application currently uses the first URL in the list.
- `network_id`: A unique identifier for the network being monitored (e.g., "mainnet", "sepolia").
- `retry_config`: Configuration for the RPC retry policy
- `block_chunk_size`: The size of the block chunk to process at once
- `polling_interval_ms`: The interval in milliseconds to poll for new blocks
- `confirmation_blocks`: Number of confirmation blocks to wait for before processing to prevent against small reorgs
- `shutdown_timeout_secs`: Graceful shutdown timeout
- `rhai`: Security configuration for Rhai script execution

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

## Project Structure

The `src` directory is organized into several modules, each with a distinct responsibility, reflecting the project's modular architecture.

-   `abi`: Handles ABI parsing, decoding, and management.
-   `config`: Manages application configuration loading and validation.
-   `engine`: The core processing and filtering logic. It contains the `BlockProcessor` and the `FilteringEngine`, which uses Rhai for script execution.
-   `http_server`: (Future) Will contain the REST API server (`axum`) for dynamic monitor management.
-   `models`: Defines the core data structures used throughout the application (e.g., `Monitor`, `BlockData`, `Transaction`).
-   `notifiers`: (Future) Will contain implementations for various notification services (e.g., Webhook, Slack).
-   `persistence`: Manages the application's state. It defines the `StateRepository` trait and includes its `SQLite` implementation.
-   `providers`: Responsible for fetching data from external sources. It defines the `DataSource` trait and includes the `EvmRpcSource` for communicating with EVM nodes.
-   `supervisor`: The top-level orchestrator that initializes and coordinates all the other components.
-   `main.rs`: The entry point of the application. It initializes the configuration and starts the `supervisor`.
-   `lib.rs`: The library crate root, where all modules are declared.
-   `test_helpers`: Contains utility functions and mock objects for use in integration and unit tests.
