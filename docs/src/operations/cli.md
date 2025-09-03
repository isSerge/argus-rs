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

The `dry-run` command is an essential tool for testing your monitors. It allows you to run your monitor configurations against a specified range of historical blocks to see what matches would have occurred. This is a safe way to verify your `filter_script` logic without running the live service.

The command will print any matches it finds to the console.

**Usage:**

```bash
cargo run --release -- dry-run --from <START_BLOCK> --to <END_BLOCK>
```

**Arguments:**

-   `--from <BLOCK>`: The starting block number (inclusive).
-   `--to <BLOCK>`: The ending block number (inclusive).

**Options:**

-   `--config-dir <PATH>`: Specifies a custom directory to load configuration files from.

**Example:**

To test your monitors against blocks 15,000,000 to 15,000,100 on the network defined in your `app.yaml`:

```bash
cargo run --release -- dry-run --from 15000000 --to 15000100
```

## Database Migrations

Database migrations are handled by `sqlx-cli`. This is not a direct subcommand of `argus` but is a critical part of the operational workflow.

Before running the application for the first time, or after any update that includes database changes, you must run the migrations:

```bash
sqlx migrate run
```
