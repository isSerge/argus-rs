# 4. All ERC20 Transfers Monitor For A Specific EOA

This example sets up a global log monitor that triggers for any `Transfer` event from any ERC20-compliant contract on the Ethereum mainnet involving specific EOA. It uses the `stdout` action for simple console output.

### Configuration Files

- [`app.yaml`](../../docs/src/user_guide/config_app.md): Basic application configuration, pointing to public RPC endpoints.
- [`monitors.yaml`](../../docs/src/user_guide/config_monitors.md): Defines the "All ERC20 Transfers (Ethereum)" monitor.
- [`actions.yaml`](../../docs/src/user_guide/config_actions.md): Defines the stdout action for console output.

### Action Options

This example uses the stdout action which prints notifications directly to the console. This is ideal for:
- Local development and testing
- Debugging monitor configurations
- Dry-run scenarios

For production use, you can configure other actions like Slack, Discord, Telegram, or webhooks. See the [Action Configuration documentation](../../docs/src/user_guide/actions_yaml.md) for all available options.

### Monitor Configuration

The `monitors.yaml` file in this example defines a single monitor. For a complete reference on monitor configuration, see the [Monitor Configuration documentation](../../docs/src/user_guide/config_monitors.md).

```yaml
monitors:
  - name: 'All ERC20 Transfers (Ethereum)'
    network: 'ethereum'
    # Monitor all logs on the network
    address: 'all' 
    # The ABI should be a generic one that matches the event you're looking for.
    # For example, a standard ERC20 ABI can decode "Transfer" events from any
    # ERC20-compliant token contract.
    abi: 'usdc'
    # Filter for all Transfer events involving a specific address
    filter_script: |
      log.name == "Transfer" 
      && log.params.to == "0xE4b8583cCB95b25737C016ac88E539D0605949e8" 
      || log.params.from == "0xE4b8583cCB95b25737C016ac88E539D0605949e8"
    actions:
      - 'Stdout ERC20 Transfers'
```

- **`name`**: A human-readable name for the monitor.
- **`network`**: Specifies the blockchain network to monitor (e.g., "ethereum"). This must match a network configured in `app.yaml`.
- **`address`**: Set to `"all"` to configure this as a global log monitor. This means the monitor will execute its script for *every* log on the blockchain that can be successfully decoded by the provided ABI, regardless of the emitting contract's address. For more details on `address` configuration, see the [Monitor Configuration documentation](../../docs/src/user_guide/config_monitors.md#monitor-fields).
- **`abi`**: The name of a generic ABI (e.g., "usdc" referring to `usdc.json`) that can decode the `Transfer` event. This is crucial for `log.name` and `log.params` to be available in the `filter_script`. For more details, see [ABI Management](../../docs/src/user_guide/config_abis.md).
- **`filter_script`**: This [Rhai script](../../docs/src/user_guide/rhai_scripts.md) defines the conditions for a match. It checks for the `Transfer` event involving specific address either as `from` or `to`. For more on `log` data, see [Rhai Data Context](../../docs/src/user_guide/rhai_context.md#the-log-object-decoded-event-log).
- **`actions`**: A list of action names (defined in `actions.yaml`) that will receive alerts when this monitor triggers. Here, it references "Stdout ERC20 Transfers".

### Action Configuration

The `actions.yaml` in this example defines a single stdout action. For a complete reference on action configuration, see the [Action Configuration documentation](../../docs/src/user_guide/config_actions.md).

```yaml
actions:
  - name: "Stdout ERC20 Transfers"
    stdout:
      message:
        title: "ERC20 Transfer Detected"
        body: |
          An ERC20 transfer was detected by monitor {{ monitor_name }}.
          - From: {{ log.params.from }}
          - To: {{ log.params.to }}
          - Value: {{ log.params.value | decimals(18) }}
          - Contract: {{ log.address }}
          - Transaction: {{ transaction_hash }}
```

-   **`name`**: A unique, human-readable name for the action. This name is referenced by monitors in their `actions` list.
-   **`stdout`**: This block configures a stdout action that prints notifications to the console.
    -   **`message`**: Defines the structure and content of the notification message. For more details on templating, see the [Action Templating documentation](../../docs/src/user_guide/action_templating.md).
        -   **`title`**: The title of the notification. Supports [Jinja2-like templating](https://docs.rs/minijinja/latest/minijinja/) to include dynamic data from the monitor match (e.g., `{{ monitor_name }}`).
        -   **`body`**: The main content of the notification. Supports [Jinja2-like templating](https://docs.rs/minijinja/latest/minijinja/) for dynamic content.

### How to Run ([Dry-Run Mode](../../docs/src/operations/cli.md#dry-run-mode))

To test this monitor against historical blocks, use the `dry-run` command with the `--config-dir` argument pointing to this example's configuration:

```bash
cargo run --release -- dry-run --from 18000000 --to 18000001 --config-dir examples/4_all_erc20_transfers_for_eoa/
```

Run with `debug` logs:

```bash
RUST_LOG=debug cargo run --release -- dry-run --from 18000000 --to 18000001 --config-dir examples/4_all_erc20_transfers_for_eoa/
```

Run with Docker image from GHCR:

```bash
docker run --rm \
  --env-file .env \
  -v "$(pwd)/examples/4_all_erc20_transfers_for_eoa:/app/configs:ro" \
  -v "$(pwd)/abis:/app/abis:ro" \
  ghcr.io/isserge/argus-rs:latest \
  dry-run --from 18000000 --to 18000010 --config-dir /app/configs
```

Replace `18000000` and `18000010` with any Ethereum block numbers to test against.

#### Expected Output

As blocks within the specified range are processed, you should see notifications printed directly to the console for ERC20 transfers involving the watched address.

Once processing is complete, you should see a summary report like this:

```
Dry Run Report
==============

Summary
-------
- Blocks Processed: 18000000 to 18000001 (2 blocks)
- Total Matches Found: 6

Matches by Monitor
------------------
- "All ERC20 Transfers (Ethereum)": 6

Notifications Dispatched
------------------------
- "Stdout ERC20 Transfers": 6
```

### How to Run (Default Mode)

Once you have verified your monitor works against historical data in `dry-run` mode, you can start it in default (live monitoring) mode. In this mode, the monitor will continuously poll for new blocks and dispatch actual notifications via the configured action when a match is found.

```bash
cargo run --release -- run --config-dir examples/4_all_erc20_transfers_for_eoa
```

Using Docker image from GHCR:

```bash
# First, create a data directory for this example
mkdir -p examples/4_all_erc20_transfers_for_eoa/data

# Run the container in detached mode
docker run --rm -d \
  --name argus_example_4 \
  --env-file .env \
  -v "$(pwd)/examples/4_all_erc20_transfers_for_eoa:/app/configs:ro" \
  -v "$(pwd)/abis:/app/abis:ro" \
  -v "$(pwd)/examples/4_all_erc20_transfers_for_eoa/data:/app/data" \
  ghcr.io/isserge/argus-rs:latest \
  run --config-dir /app/configs
```
