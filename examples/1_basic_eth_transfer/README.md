# 1. Basic ETH Transfer Monitor

This example sets up a monitor that triggers when a transaction with a value
greater than 10 ETH is detected on the Ethereum mainnet. It uses the `stdout`
action for simple console output.

### Configuration Files

- [`app.yaml`](../../docs/src/user_guide/config_app.md): Basic application configuration, pointing to public RPC endpoints.
- [`monitors.yaml`](../../docs/src/user_guide/config_monitors.md): Defines the "Large ETH Transfers" monitor.
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
  - name: 'Large ETH Transfers'
    network: 'ethereum'
    filter_script: |
      tx.value > ether(10)
    actions:
      - 'stdout-action'
```

- **`name`**: A human-readable name for the monitor.
- **`network`**: Specifies the blockchain network to monitor (e.g., "ethereum").
  This must match a network configured in `app.yaml`.
- **`filter_script`**: This is a [Rhai script](../../docs/src/user_guide/rhai_scripts.md) that defines the conditions under
  which the monitor will trigger. In this example, `tx.value > ether(10)` checks
  if the transaction's value is greater than 10 ETH. The [`ether()`](../../docs/src/user_guide/rhai_helpers.md#ethervalue) function is a
  convenient wrapper, which handles `BigInt` conversions.
- **`actions`**: A list of action names (defined in `actions.yaml`) that
  will receive alerts when this monitor triggers. Here, it references "stdout-action".

### Action Configuration

The `actions.yaml` in this example defines a `stdout` action that prints notifications to the console. For a complete reference on action configuration, see the [Action Configuration documentation](../../docs/src/user_guide/actions_yaml.md).

```yaml
actions:
  - name: 'stdout-action'
    stdout:
      message:
        title: 'Large ETH Transfer Detected'
        body: |
          A transfer of over 10 ETH was detected by monitor {{ monitor_name }}.
          - From: `{{ tx.from }}`
          - To: `{{ tx.to }}`
          - Value: `{{ tx.value | ether }} ETH`
          [View on Etherscan](https://etherscan.io/tx/{{ transaction_hash }})
```

- **`name`**: A unique, human-readable name for the action. This name is
  referenced by monitors in their `actions` list.
- **`stdout`**: This block configures a stdout action that prints to the console.
  - **`message`**: Defines the structure and content of the notification
    message. For more details on templating, see the [Action Templating documentation](../../docs/src/user_guide/action_templating.md).
    - **`title`**: The title of the notification. Supports
      [Jinja2-like templating](https://docs.rs/minijinja/latest/minijinja/) to
      include dynamic data from the monitor match (e.g., `{{ monitor_name }}`).
    - **`body`**: The main content of the notification. Supports
      [Jinja2-like templating](https://docs.rs/minijinja/latest/minijinja/) for
      dynamic content.

### How to Run ([Dry-Run Mode](../../docs/src/operations/cli.md#dry-run-mode))

To test this monitor against historical blocks, use the `dry-run` command with
the `--config-dir` argument pointing to this example's configuration:

```bash
cargo run --release -- dry-run --from 23159290 --to 23159291 --config-dir examples/1_basic_eth_transfer/
```

Run with `debug` logs:

```bash
RUST_LOG=debug cargo run --release -- dry-run --from 23159290 --to 23159291 --config-dir examples/1_basic_eth_transfer/
```

Run with Docker image from GHCR:

```bash
docker run --rm \
  --env-file .env \
  -v "$(pwd)/examples/1_basic_eth_transfer:/app/configs:ro" \
  -v "$(pwd)/abis:/app/abis:ro" \
  ghcr.io/isserge/argus-rs:latest \
  dry-run --from 23159290 --to 23159291 --config-dir /app/configs
```

Replace `23159290` and `23159291` with any Ethereum block numbers to test
against.

#### Expected Output

As blocks within the specified range are processed, you should see notifications
printed to the console via the stdout action. Each notification will be
formatted according to the message template defined in the action configuration.

For example, when a large ETH transfer is detected, you'll see output like:
```
Large ETH Transfer Detected
A transfer of over 10 ETH was detected by monitor Large ETH Transfers.
- From: `0x30F2864e7bf6E89a3955217C78c8689594228940`
- To: `0xCFFAd3200574698b78f32232aa9D63eABD290703`
- Value: `13.859958 ETH`
[View on Etherscan](https://etherscan.io/tx/0x92a19bc7912f993ed94faa3ea102f4fd244aaf78f5439071bd1126ab419f2ce6)
```

After `dry-run` processing is complete, you should see a summary report:

```
Dry Run Report
==============

Summary
-------
- Blocks Processed: 23159290 to 23159291 (2 blocks)
- Total Matches Found: 2

Matches by Monitor
------------------
- "Large ETH Transfers": 2

Notifications Dispatched
------------------------
- "stdout-action": 2
```

### How to Run (Default Mode)

Once you have verified your monitor works against historical data in `dry-run`
mode, you can start it in default (live monitoring) mode. In this mode, the
monitor will continuously poll for new blocks and dispatch actual notifications
via the configured action when a match is found.

```bash
cargo run --release -- run --config-dir examples/1_basic_eth_transfer/
```

Using Docker image from GHCR:

```bash
# First, create a data directory for this example
mkdir -p examples/1_basic_eth_transfer/data

# Run the container in detached mode
docker run --rm -d \
  --name argus_example_1 \
  --env-file .env \
  -v "$(pwd)/examples/1_basic_eth_transfer:/app/configs:ro" \
  -v "$(pwd)/abis:/app/abis:ro" \
  -v "$(pwd)/examples/1_basic_eth_transfer/data:/app/data" \
  ghcr.io/isserge/argus-rs:latest \
  run --config-dir /app/configs
```
