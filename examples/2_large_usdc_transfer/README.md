# 2. Large USDC Transfer Monitor

This example sets up a monitor that triggers when a `Transfer` event with a
value greater than 1,000,000 USDC is detected from the USDC contract on the
Ethereum mainnet. It uses the `stdout` action for simple console output.

### Configuration Files

- [`app.yaml`](../../docs/src/user_guide/config_app.md): Basic application configuration, pointing to public RPC endpoints.
- [`monitors.yaml`](../../docs/src/user_guide/config_monitors.md): Defines the "Large USDC Transfers" monitor.
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
  - name: 'Large USDC Transfers'
    network: 'ethereum'
    address: '0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48'
    abi: 'usdc'
    filter_script: |
      log.name == "Transfer" && log.params.value > usdc(1_000_000)
    actions:
      - 'stdout-action'
```

- **`name`**: A human-readable name for the monitor.
- **`network`**: Specifies the blockchain network to monitor (e.g., "ethereum").
  This must match a network configured in `app.yaml`.
- **`address`**: The contract address of the USDC token on Ethereum mainnet.
  This ensures the monitor only processes events from this specific contract. For more details on `address` configuration, see the [Monitor Configuration documentation](../../docs/src/user_guide/config_monitors.md#monitor-fields).
- **`abi`**: The name of the ABI (Application Binary Interface) to use for
  decoding contract events. Here, "usdc" refers to the `usdc.json` file in the
  `abis/` directory. This is crucial for `log.name` and `log.params` to be
  available in the `filter_script`. For more details, see [ABI Management](../../docs/src/user_guide/config_abis.md).
- **`filter_script`**: This [Rhai script](../../docs/src/user_guide/rhai_scripts.md) defines the conditions for a match.
  `log.name == "Transfer"` checks for the `Transfer` event, and
  `log.params.value > usdc(1_000_000)` checks if the `value` parameter of that
  event is greater than 1 million USDC. The [`usdc()`](../../docs/src/user_guide/rhai_helpers.md#usdcvalue) function is a convenient
  wrapper for handling USDC denominations. For more on `log` data, see [Rhai Data Context](../../docs/src/user_guide/rhai_context.md#the-log-object-decoded-event-log).
- **`actions`**: A list of action names (defined in `actions.yaml`) that
  will receive alerts when this monitor triggers. Here, it references "stdout-action".

### Action Configuration

The `actions.yaml` in this example defines a `stdout` action that prints notifications to the console. For a complete reference on action configuration, see the [Action Configuration documentation](../../docs/src/user_guide/actions_yaml.md).

```yaml
actions:
  - name: 'stdout-action'
    stdout:
      message:
        title: 'Large USDC Transfer Detected'
        body: |
          A transfer of over 1,000,000 USDC was detected by monitor {{ monitor_name }}.
          - From: `{{ log.params.from }}`
          - To: `{{ log.params.to }}`
          - Value: `{{ log.params.value | usdc }} USDC`
          [View on Etherscan](https://etherscan.io/tx/{{ transaction_hash }})
```

- **`name`**: A unique, human-readable name for the action. This name is
  referenced by monitors in their `actions` list.
- **`stdout`**: This block configures a stdout action that prints to the console.
  - **`message`**: Defines the structure and content of the notification
    message. For more details on templating, see the [action Templating documentation](../../docs/src/user_guide/action_templating.md).
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
cargo run --release -- dry-run --from 23159291 --to 23159292 --config-dir examples/2_large_usdc_transfer/
```

Run with `debug` logs:

```bash
RUST_LOG=debug cargo run --release -- dry-run --from 23159291 --to 23159292 --config-dir examples/2_large_usdc_transfer/
```

Run with Docker image from GHCR:

```bash
docker run --rm \
  --env-file .env \
  -v "$(pwd)/examples/2_large_usdc_transfer:/app/configs:ro" \
  -v "$(pwd)/abis:/app/abis:ro" \
  ghcr.io/isserge/argus-rs:latest \
  dry-run --from 23159291 --to 23159292 --config-dir /app/configs
```

Replace `23159291` and `23159292` with any Ethereum block numbers to test
against.

#### Expected Output

As blocks within the specified range are processed, you should see notifications
printed to the console via the stdout action. Each notification will be
formatted according to the message template defined in the action configuration.

For example, when a large USDC transfer is detected, you'll see output like:
```
Large USDC Transfer Detected
A transfer of over 1,000,000 USDC was detected by monitor Large USDC Transfers.
- From: `0xEae7380dD4CeF6fbD1144F49E4D1e6964258A4F4`
- To: `0xEe7aE85f2Fe2239E27D9c1E23fFFe168D63b4055`
- Value: `18.61665 USDC`
[View on Etherscan](https://etherscan.io/tx/0x583af129ea78623a363d8b0f582f220a14713ba4717771e30ec4408239991d0f)
```

After `dry-run` processing is complete, you should see a summary report:

```
Dry Run Report
==============

Summary
-------
- Blocks Processed: 23159291 to 23159292 (2 blocks)
- Total Matches Found: 2

Matches by Monitor
------------------
- "Large USDC Transfers": 2

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
cargo run --release -- run --config-dir examples/2_large_usdc_transfer/
```

Using Docker image from GHCR:

```bash
# First, create a data directory for this example
mkdir -p examples/2_large_usdc_transfer/data

# Run the container in detached mode
docker run --rm -d \
  --name argus_example_2 \
  --env-file .env \
  -v "$(pwd)/examples/2_large_usdc_transfer:/app/configs:ro" \
  -v "$(pwd)/abis:/app/abis:ro" \
  -v "$(pwd)/examples/2_large_usdc_transfer/data:/app/data" \
  ghcr.io/isserge/argus-rs:latest \
  run --config-dir /app/configs
```

