# Argus: monitoring and event-sourcing for EVM

[![📖 Docs](https://img.shields.io/badge/📖_Docs-gray)](https://isserge.github.io/argus-rs/)

![alt text](banner.png)

Argus Panoptes (“the All-Seeing”) was the sleepless giant of Greek mythology. With a hundred eyes, he could watch without rest, making him a perfect guardian.

> **Note:** This project is under active development and is not yet ready for production use. The API and architecture are subject to change.

Argus is a next-generation, open-source, self-hosted monitoring tool for EVM chains. It's designed to be API-first, highly reliable, and deeply flexible, serving as a foundational piece of infrastructure for any EVM project.

## Features

### Current Features

-   **Real-Time EVM Monitoring**: Connects to an EVM RPC endpoint to monitor new blocks in real time, processing transactions and logs as they occur.
-   **Flexible Filtering with Rhai**: Uses the embedded [Rhai](https://rhai.rs) scripting language to create highly specific and powerful filters for any on-chain event, from simple balance changes to complex DeFi interactions.
-   **EVM Value Wrappers**: Convenient functions like `ether`, `gwei`, and `usdc` for handling common token denominations, plus a generic `decimals` function for custom tokens. This makes filter scripts more readable and less error-prone.
-   **Multi-Channel Notifications**: Supports webhook, Slack, Discord, and Telegram notifications, allowing for easy integration with your favorite services.
-   **Advanced Notification Policies**: Support alert aggregation and throttling to reduce noise.
-   **Kafka Integration**: Stream filtered events into Kafka topics for scalable data pipelines.
-   **Stateful Processing**: Tracks its progress in a local SQLite database, allowing it to stop and resume from where it left off without missing any blocks.
-   **CLI Dry-Run Mode**: A `dry-run` command allows you to test your monitor against a range of historical blocks to ensure it works as expected before deploying it live.
-   **Docker Support**: Comes with a multi-stage `Dockerfile` and `docker compose.yml` for easy, portable deployments.

### Future Features (Planned)

-   **Dynamic Configuration via REST API**: Add, update, and remove monitors on the fly via a REST API, without any downtime.
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

The application's behavior is primarily configured through three YAML files located in the `configs` directory: `app.yaml`, `monitors.yaml`, and `actions.yaml`. You can specify an alternative directory using the `--config-dir` CLI argument.

### `app.yaml`

This file contains the main application settings, such as the database connection, RPC endpoints, and performance tuning parameters. For a complete list of parameters and their descriptions, see the [app.yaml documentation](./docs/src/user_guide/config_app.md).

**Example `app.yaml`**
```yaml
database_url: "sqlite:data/monitor.db"
rpc_urls:
  - "https://eth.llamarpc.com"
  - "https://1rpc.io/eth"
  - "https://rpc.mevblocker.io"
network_id: "mainnet"

# Start 100 blocks behind the chain tip to avoid issues with block reorganizations.
initial_start_block: -100

block_chunk_size: 5
polling_interval_ms: 10000
confirmation_blocks: 12
notification_channel_capacity: 1024
abi_config_path: abis/
```

### `monitors.yaml`

This file is where you define *what* you want to monitor on the blockchain. Each monitor specifies a network, an optional contract address, and a Rhai filter script. If your script needs to inspect event logs (i.e., access the `log` variable), you must also provide the **name** of the contract's ABI. This name should correspond to a `.json` file (without the `.json` extension) located in the `abis/` directory (or the directory configured for ABIs in `app.yaml`). For a complete list of parameters and their descriptions, see the [monitors.yaml documentation](./docs/src/user_guide/config_monitors.md).

See [`configs/monitors.example.yaml`](./configs/monitors.example.yaml) for more detailed examples.

**Example `monitors.yaml`**
```yaml
monitors:
  # Monitor for large native ETH transfers (no ABI needed)
  - name: "Large ETH Transfers"
    network: "mainnet"
    # No address means it runs on every transaction.
    # This type of monitor inspects transaction data directly.
    filter_script: |
      tx.value > ether(10)
    # actions are used to send notifications when the monitor triggers.
    actions:
      # This monitor will use the "my-generic-webhook" action defined in `actions.yaml`.
      - "my-generic-webhook"

  # Monitor for large USDC transfers (ABI is required)
  - name: "Large USDC Transfers"
    network: "mainnet"
    address: "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48"
    # The name of the ABI (Application Binary Interface) for the contract being monitored
    abi: "usdc"
    filter_script: |
      log.name == "Transfer" && log.params.value > usdc(1_000_000)
    # actions can be used to send notifications when the monitor triggers.
    actions:
      # This monitor will use the "slack-notifications" action defined in `actions.yaml`.
      - "slack-notifications"
```

### `actions.yaml`

This file defines *how* you want to be notified when a monitor finds a match. You can configure various notification channels like webhooks, Slack, or Discord. For a complete list of parameters and their descriptions, see the [actions.yaml documentation](./docs/src/user_guide/config_actions.md).

See [`configs/actions.example.yaml`](./configs/actions.example.yaml) for more detailed examples.

**Example `actions.yaml`**
```yaml
actions:
  - name: "my-generic-webhook"
    webhook:
      url: "https://my-service.com/webhook-endpoint"
      message:
        title: "New Transaction Alert: {{ monitor_name }}"
        body: |
          A new event was detected on contract {{ log.address }}.
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

## Examples

This project includes a variety of examples to help you get started. Each example is self-contained and demonstrates a specific use case.

For a full list of examples, see the [Examples README](./examples/README.md).

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
    cargo run --release -- dry-run --from <START> --to <END>

    # This runs the main monitoring service with custom config directory
    cargo run --release -- run --config-dir <CUSTOM CONFIG DIR>

    # To run a dry run with custom config directory
    cargo run --release -- dry-run --from <START> --to <END> --config-dir <CUSTOM CONFIG DIR>
    ```

## Running with Docker

The easiest way to run Argus is with Docker Compose. This method handles the database, configuration, and application lifecycle for you.

1.  **Clone the repository:**
    ```bash
    git clone https://github.com/isSerge/argus-rs
    cd argus-rs
    ```

2.  **Configure Secrets:**
    Copy the example environment file and fill in your action secrets (e.g., Telegram token, Slack webhook URL).
    ```bash
    cp .env.example .env
    # Now, edit .env with your secrets
    ```

3.  **Create Data Directory:**
    Create a local directory to persist the database.
    ```bash
    mkdir -p data
    ```

4.  **Run the Application:**
    Start the application in detached mode.
    ```bash
    docker compose up -d
    ```

You can view logs with `docker compose logs -f` and stop the application with `docker compose down`. For more details, see the [Deployment with Docker documentation](./docs/src/operations/deployment.md).

## Project Structure

The `src` directory is organized into several modules, each with a distinct responsibility.

-   `abi`: Handles ABI parsing, decoding, and management.
-   `config`: Manages application configuration loading and validation.
-   `engine`: The core processing and filtering logic, including the Rhai script executor.
-   `http_client`: Provides a retryable HTTP client and a pool for managing clients.
-   `http_server`: (Future) Will contain the REST API server for dynamic monitor management.
-   `initialization`: Loads initial data (monitors, actions, ABIs) at startup.
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
