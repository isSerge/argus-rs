# 3. WETH Deposit Monitor

This example sets up a monitor that triggers when a `Deposit` event is detected
from the WETH contract on the Ethereum mainnet. It uses the `stdout` notifier
for simple console output.

### Configuration Files

- [`app.yaml`](../../docs/src/user_guide/config_app.md): Basic application configuration, pointing to public RPC endpoints.
- [`monitors.yaml`](../../docs/src/user_guide/config_monitors.md): Defines the "WETH Deposits" monitor.
- [`notifiers.yaml`](../../docs/src/user_guide/config_notifiers.md): Defines the stdout notifier for console output.

### Notifier Options

This example uses the stdout notifier which prints notifications directly to the console. This is ideal for:
- Local development and testing
- Debugging monitor configurations
- Dry-run scenarios

For production use, you can configure other notifiers like Slack, Discord, Telegram, or webhooks. See the [Notifier Configuration documentation](../../docs/src/user_guide/notifiers_yaml.md) for all available options.

### Monitor Configuration

The `monitors.yaml` file in this example defines a single monitor. For a complete reference on monitor configuration, see the [Monitor Configuration documentation](../../docs/src/user_guide/config_monitors.md).

```yaml
monitors:
  - name: 'WETH Deposits'
    network: 'ethereum'
    address: '0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2'
    abi: 'weth'
    filter_script: |
      log.name == "Deposit" && tx.value > ether(5)
    notifiers:
      - 'stdout-notifier'
```

- **`name`**: A human-readable name for the monitor.
- **`network`**: Specifies the blockchain network to monitor (e.g., "ethereum").
  This must match a network configured in `app.yaml`.
- **`address`**: The contract address of the WETH token on Ethereum mainnet.
  This ensures the monitor only processes events from this specific contract. For more details on `address` configuration, see the [Monitor Configuration documentation](../../docs/src/user_guide/config_monitors.md#monitor-fields).
- **`abi`**: The name of the ABI (Application Binary Interface) to use for
  decoding contract events. Here, "weth" refers to the `weth.json` file in the
  `abis/` directory. This is crucial for `log.name` and `log.params` to be
  available in the `filter_script`. For more details, see [ABI Management](../../docs/src/user_guide/config_abis.md).
- **`filter_script`**: This [Rhai script](../../docs/src/user_guide/rhai_scripts.md) defines the conditions for a match.
  `log.name == "Deposit"` checks for the `Deposit` event, `tx.value > ether(5)`
  checks if the transaction's value is greater than 5 ETH. The [`ether()`](../../docs/src/user_guide/rhai_helpers.md#ethervalue) function is a
  convenient wrapper, which handles `BigInt` conversions. For more on `log` and `tx` data, see [Rhai Data Context](../../docs/src/user_guide/rhai_context.md).
- **`notifiers`**: A list of notifier names (defined in `notifiers.yaml`) that
  will receive alerts when this monitor triggers. Here, it references "stdout-notifier".

### Notifier Configuration

The `notifiers.yaml` in this example defines a `stdout` notifier that prints notifications to the console. For a complete reference on notifier configuration, see the [Notifier Configuration documentation](../../docs/src/user_guide/notifiers_yaml.md).

```yaml
notifiers:
  - name: 'stdout-notifier'
    stdout:
      message:
        title: 'WETH Deposit Detected'
        body: |
          A WETH deposit was detected by monitor {{ monitor_name }}.
          - From: `{{ log.params.dst }}`
          - Value: `{{ log.params.wad | ether }} WETH`
          [View on Etherscan](https://etherscan.io/tx/{{ transaction_hash }})
```

- **`name`**: A unique, human-readable name for the notifier. This name is
  referenced by monitors in their `notifiers` list.
- **`stdout`**: This block configures a stdout notifier that prints to the console.
  - **`message`**: Defines the structure and content of the notification
    message. For more details on templating, see the [Notifier Templating documentation](../../docs/src/user_guide/notifier_templating.md).
    - **`title`**: The title of the notification. Supports
      [Jinja2-like templating](https://docs.rs/minijinja/latest/minijinja/) to
      include dynamic data from the monitor match (e.g., `{{ monitor_name }}`).
    - **`body`**: The main content of the notification. In this example we use
      event log fields `dst` (the address that initiated the deposit) and `wad`
      (the amount of WETH deposited). For more on `log` data, see [Rhai Data Context](../../docs/src/user_guide/rhai_context.md#the-log-object-decoded-event-log).

### How to Run ([Dry-Run Mode](../../docs/src/operations/cli.md#dry-run-mode))

To test this monitor against historical blocks, use the `dry-run` command with
the `--config-dir` argument pointing to this example's configuration:

```bash
cargo run --release -- dry-run --from 18000003 --to 18000004 --config-dir examples/3_weth_deposit/
```

Run with `debug` logs:

```bash
RUST_LOG=debug cargo run --release -- dry-run --from 18000003 --to 18000004 --config-dir examples/3_weth_deposit/
```

Run with Docker image from GHCR:

```bash
docker run --rm \
  --env-file .env \
  -v "$(pwd)/examples/3_weth_deposit:/app/configs:ro" \
  -v "$(pwd)/abis:/app/abis:ro" \
  ghcr.io/isserge/argus-rs:latest \
  dry-run --from 18000003 --to 18000004 --config-dir /app/configs
```

Replace `18000003` and `18000004` with any Ethereum block numbers to test
against.

#### Expected Output

As blocks within the specified range are processed, you should see notifications
printed to the console via the stdout notifier. Each notification will be
formatted according to the message template defined in the notifier configuration.

For example, when a WETH deposit is detected, you'll see output like:
```
WETH Deposit Detected
A WETH deposit was detected by monitor WETH Deposits.
- From: `0x3fC91A3afd70395Cd496C647d5a6CC9D4B2b7FAD`
- Value: `7.0 WETH`
[View on Etherscan](https://etherscan.io/tx/0xf180607da1c35e13bdb91ac8006337774812c34075ed1cdd9a7765f5197b4882)
```

After `dry-run` processing is complete, you should see a summary report:

```
Dry Run Report
==============

Summary
-------
- Blocks Processed: 18000003 to 18000004 (2 blocks)
- Total Matches Found: 2

Matches by Monitor
------------------
- "WETH Deposits": 2

Notifications Dispatched
------------------------
- "stdout-notifier": 2
```

### How to Run (Default Mode)

Once you have verified your monitor works against historical data in `dry-run`
mode, you can start it in default (live monitoring) mode. In this mode, the
monitor will continuously poll for new blocks and dispatch actual notifications
via the configured notifier when a match is found.

```bash
cargo run --release -- run --config-dir examples/3_weth_deposit/
```

Using Docker image from GHCR:

```bash
# First, create a data directory for this example
mkdir -p examples/3_weth_deposit/data

# Run the container in detached mode
docker run --rm -d \
  --name argus_example_3 \
  --env-file .env \
  -v "$(pwd)/examples/3_weth_deposit:/app/configs:ro" \
  -v "$(pwd)/abis:/app/abis:ro" \
  -v "$(pwd)/examples/3_weth_deposit/data:/app/data" \
  ghcr.io/isserge/argus-rs:latest \
  run --config-dir /app/configs
```
