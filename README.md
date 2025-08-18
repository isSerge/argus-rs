# Argus EVM Monitor

> **Note:** This project is under active development and is not yet ready for production use. The API and architecture are subject to change.

Argus is a next-generation, open-source, self-hosted monitoring tool for EVM chains. It's designed to be API-first, highly reliable, and deeply flexible, serving as a foundational piece of infrastructure for any serious EVM project.

## Features

### Current Features

-   **Real-Time EVM Monitoring**: Connects to an EVM RPC endpoint to monitor new blocks in real time, processing transactions and logs as they occur.
-   **Flexible Filtering with Rhai**: Uses the embedded [Rhai](https://rhai.rs) scripting language to create highly specific and powerful filters for any on-chain event, from simple balance changes to complex DeFi interactions.
-   **Basic Notifications**: Supports webhook notifications, allowing for easy integration with services like Slack, Discord, or custom automation workflows.
-   **Stateful Processing**: Tracks its progress in a local SQLite database, allowing it to stop and resume from where it left off without missing any blocks.
-   **CLI Dry-Run Mode**: A `dry-run` command allows you to test your monitor against a range of historical blocks to ensure it works as expected before deploying it live.

### Future Features (Planned)

-   **Dynamic Configuration via REST API**: Add, update, and remove monitors on the fly via a REST API, without any downtime.
-   **Advanced Notifications**: Support alert aggregation and throttling to reduce noise.
-   **Stateful Filtering**: The ability for a filter to remember past events and make decisions based on a time window (e.g., "alert if an address withdraws more than 5 times in 10 minutes").
-   **Data Enrichment & Cross-Contract Checks**: Make monitors "smarter" by allowing them to fetch external data (e.g., from a price API) or check the state of another contract as part of their filtering logic.
-   **Automatic ABI Fetching**: Automatically fetch contract ABIs from public registries like Etherscan, reducing the amount of manual configuration required.
-   **Web UI/Dashboard**: A simple web interface for managing monitors and viewing a live feed of recent alerts.

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

The application's behavior is primarily configured through three YAML files located in the `configs` directory: `app.yaml`, `monitors.yaml`, and `notifiers.yaml`. You can specify an alternative directory using the `--config-dir` CLI argument.

### `app.yaml`

This file contains the main application settings, such as the database connection, RPC endpoints, and performance tuning parameters.

**Example `app.yaml`**
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
notification_channel_capacity: 1024
```

### `monitors.yaml`

This file is where you define *what* you want to monitor on the blockchain. Each monitor specifies a network, an optional contract address, and a Rhai filter script. If your script needs to inspect event logs (i.e., access the `log` variable), you must also provide a path to the contract's ABI file.

**Example `monitors.yaml`**
```yaml
monitors:
  # Monitor for large native ETH transfers (no ABI needed)
  - name: "Large ETH Transfers"
    network: "mainnet"
    # No address means it runs on every transaction.
    # This type of monitor inspects transaction data directly.
    filter_script: |
      tx.value > bigint("10000000000000000000") # 10 ETH

  # Monitor for large USDC transfers (ABI is required)
  - name: "Large USDC Transfers"
    network: "mainnet"
    address: "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48"
    # The ABI file is needed to decode event logs.
    abi: "abis/usdc.json"
    filter_script: |
      log.name == "Transfer" && log.params.value > bigint("1000000000000") # 1M USDC
```

### `notifiers.yaml`

This file defines *how* you want to be notified when a monitor finds a match. You can configure various notification channels like webhooks, Slack, or Discord.

**Example `notifiers.yaml`**
```yaml
notifiers:
  - name: "my-generic-webhook"
    webhook:
      url: "https://my-service.com/webhook-endpoint"
      message:
        title: "New Transaction Alert: {{ monitor_name }}"
        body: |
          A new event was detected on contract {{ contract_address }}.
          - **Block Number**: {{ block_number }}
          - **Transaction Hash**: {{ transaction_hash }}

  - name: "slack-notifications"
    slack:
      slack_url: "https://hooks.slack.com/services/T00000000/B00000000/XXXXXXXXXXXXXXXXXXXXXXXX"
      message:
        title: "Large USDC Transfer Detected"
        body: |
          A transfer of over 1,000,000 USDC was detected.
          <https://etherscan.io/tx/{{ transaction_hash }}|View on Etherscan>
```

### Configuration Parameters (`app.yaml`)

| Parameter | Description | Default |
| :--- | :--- | :--- |
| `database_url` | The connection string for the SQLite database. | (Required) |
| `rpc_urls` | A list of RPC endpoint URLs for the EVM network. | (Required) |
| `network_id` | A unique identifier for the network being monitored (e.g., "mainnet"). | (Required) |
| `block_chunk_size` | The number of blocks to fetch and process in a single batch. | `5` |
| `polling_interval_ms` | The interval in milliseconds to poll for new blocks. | `10000` |
| `confirmation_blocks` | Number of blocks to wait for before processing to protect against reorgs. | `12` |
| `notification_channel_capacity` | The capacity of the internal channel for sending notifications. | `1024` |
| `shutdown_timeout_secs` | The maximum time in seconds to wait for a graceful shutdown. | `30` |
| `retry_config.max_retry` | The maximum number of retries for a failing RPC request. | `10` |
| `retry_config.backoff_ms` | The initial backoff delay in milliseconds for RPC retries. | `1000` |
| `retry_config.compute_units_per_second` | The number of compute units per second to allow (for rate limiting). | `100` |
| `rhai.max_operations` | Maximum number of operations a Rhai script can perform. | `100000` |
| `rhai.max_call_levels` | Maximum function call nesting depth in a Rhai script. | `10` |
| `rhai.max_string_size` | Maximum size of strings in characters in a Rhai script. | `8192` |
| `rhai.max_array_size` | Maximum number of array elements in a Rhai script. | `1000` |
| `rhai.execution_timeout` | Maximum execution time per Rhai script in milliseconds. | `5000` |

## Logging

Argus uses the `tracing` crate for structured logging. You can control the verbosity of the logs using the `RUST_LOG` environment variable.

Examples:
- `RUST_LOG=info cargo run --release`: Only shows `INFO` level messages and above.
- `RUST_LOG=debug cargo run --release`: Shows `DEBUG` level messages and above.
- `RUST_LOG=argus=trace cargo run --release`: Shows `TRACE` level messages and above specifically for the `argus` crate.

For more detailed control, refer to the `tracing-subscriber` documentation on `EnvFilter`.

## Database Setup

The application uses `sqlx` to manage database migrations. The state is stored in a local SQLite database file, configured via the `database_url` in `app.yaml`.

The database file will be created automatically on the first run if it doesn't exist.

1.  **Ensure `database_url` is set** in your `app.yaml`.

2.  **Run migrations:**
    This command will create the necessary tables in the database.
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
    # This runs the main monitoring service
    cargo run --release -- run

    # To run a dry run for a block range
    cargo run --release -- dry-run --from-block <START> --to-block <END>

    # This runs the main monitoring service with custom config directory
    cargo run --release -- run --config-dir <CUSTOM CONFIG DIR>

    # To run a dry run with custom config directory
    cargo run --release -- dry-run --from-block <START> --to-block <END> --config-dir <CUSTOM CONFIG DIR>
    ```

## Project Structure

The `src` directory is organized into several modules, each with a distinct responsibility.

-   `abi`: Handles ABI parsing, decoding, and management.
-   `config`: Manages application configuration loading and validation.
-   `engine`: The core processing and filtering logic, including the Rhai script executor.
-   `http_client`: Provides a retryable HTTP client and a pool for managing clients.
-   `http_server`: (Future) Will contain the REST API server for dynamic monitor management.
-   `initialization`: Loads initial data (monitors, notifiers, ABIs) at startup.
-   `models`: Defines the core data structures (e.g., `Monitor`, `BlockData`, `Transaction`).
-   `notification`: Manages sending notifications to services like webhooks.
-   `persistence`: Manages the application's state via the `StateRepository` trait.
-   `providers`: Fetches data from external sources like EVM nodes.
-   `supervisor`: The top-level orchestrator that initializes and coordinates all services.
-   `test_helpers`: Utilities and mock objects for tests.
-   `main.rs`: The application's entry point, handling CLI parsing and startup.
-   `lib.rs`: The library crate root.

## Contributing

Contributions are welcome! Please see our [Contributing Guide](CONTRIBUTING.md) for more details on how to get started, our development process, and coding standards.
