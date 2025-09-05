# Command-Line Interface (CLI)

Argus is primarily a long-running service, but it also provides a command-line interface for common operations, such as running the main service, testing monitors, and managing the database.

## Main Commands

You can see the available commands by running `cargo run -- --help`.

```
Usage: argus <COMMAND>

Commands:
  run      Starts the main monitoring service
  dry-run  Runs a dry run of the monitors against a range of historical blocks
  help     Print this message or the help of the given subcommand(s)
```

### `run`

This is the main command to start the Argus monitoring service.

```bash
cargo run --release -- run
```

This command will:
1.  Load the configuration from the `configs/` directory.
2.  Connect to the database and apply any pending migrations.
3.  Connect to the configured RPC endpoints.
4.  Start polling for new blocks and processing them against your monitors.

**Options:**

-   `--config-dir <PATH>`: Specifies a custom directory to load configuration files from.

    ```bash
    cargo run --release -- run --config-dir /path/to/my/configs
    ```

### `dry-run`

The `dry-run` command is an essential tool for testing and validating your monitor configurations and Rhai filter scripts against historical blockchain data. It allows you to simulate the monitoring process over a specified range of blocks without affecting the live service or making persistent database changes.

**How it Works:**

1.  **One-Shot Execution**: The command initializes all necessary application services (data source, block processor, filtering engine, etc.) in a temporary, one-shot mode.
2.  **In-Memory Database**: It uses a temporary, in-memory SQLite database for state management, ensuring that no persistent changes are made to your actual database.
3.  **Block Processing**: It fetches and processes blocks in batches (defaulting to 50 blocks per batch) within the specified `--from` and `--to` range.
4.  **Script Evaluation**: For each transaction and log in the processed blocks, it evaluates your monitor's `filter_script`.
5.  **Real Notifications (Test Mode)**: Any matches found will trigger *real* notifications to your configured notifiers. During development, it's highly recommended to configure your notifiers to point to test endpoints (e.g., [Webhook.site](https://webhook.site/)) to avoid sending unwanted alerts.
6.  **JSON Report**: After processing the entire block range, the command prints a comprehensive JSON array of all detected `MonitorMatch`es to standard output. This output is invaluable for verifying your script logic and understanding what events would trigger alerts.

**Usage:**

```bash
cargo run --release -- dry-run --from <START_BLOCK> --to <END_BLOCK> [--config-dir <PATH>]
```

**Arguments:**

*   `--from <BLOCK>`: The starting block number for the dry run (inclusive).
*   `--to <BLOCK>`: The ending block number for the dry run (inclusive).

**Options:**

*   `--config-dir <PATH>`: (Optional) Specifies a custom directory to load configuration files from. Defaults to `configs/`.

**Example:**

To test your monitors against blocks 15,000,000 to 15,000,100 on the network defined in your [`app.yaml`](../user_guide/app_yaml.md):

```bash
cargo run --release -- dry-run --from 15000000 --to 15000100
```

For a practical example of using `dry-run` to test a monitor, refer to the [Basic ETH Transfer Monitor example](../examples/1_basic_eth_transfer/README.md#how-to-run-dry-run-mode).

## Database Migrations

Database migrations are handled by `sqlx-cli`. This is not a direct subcommand of `argus` but is a critical part of the operational workflow.

Before running the application for the first time, or after any update that includes database changes, you must run the migrations:

```bash
sqlx migrate run
```
