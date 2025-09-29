# 8. High Priority Fee

This example demonstrates how to monitor for transactions with unusually high priority fees, which can be an indicator of MEV (Maximal Extractable Value) activity, front-running, or other urgent on-chain actions. It uses the `stdout` action for simple console output.

### Configuration Files

- [`app.yaml`](../../docs/src/user_guide/config_app.md): Basic application configuration, pointing to public RPC endpoints.
- [`monitors.yaml`](../../docs/src/user_guide/config_monitors.md): Defines the "High Priority Fee Alert" monitor.
- [`actions.yaml`](../../docs/src/user_guide/config_actions.md): Defines the stdout action for console output.

### Action Options

This example uses the stdout action which prints notifications directly to the console. This is ideal for:
- Local development and testing
- Debugging monitor configurations and MEV detection
- Dry-run scenarios

For production use, you can configure other actions like Slack, Discord, Telegram, or webhooks. See the [Action Configuration documentation](../../docs/src/user_guide/actions_yaml.md) for all available options.

### Monitor Configuration

The `monitors.yaml` file defines a monitor that triggers for any transaction with a `max_priority_fee_per_gas` greater than 100 Gwei.

```yaml
monitors:
  - name: "High Priority Fee Alert"
    network: "ethereum"
    filter_script: |
      // A fee over 150 is highly unusual and indicates a strong desire for priority inclusion.
      let priority_fee_threshold = gwei(150);

      tx.max_priority_fee_per_gas > priority_fee_threshold
    actions:
      - "Stdout High Priority Fee"
```

- **`filter_script`**: This [Rhai script](../../docs/src/user_guide/rhai_scripts.md) accesses the EIP-1559 fee data from the `tx` object.
    - **`tx.max_priority_fee_per_gas`**: This field contains the "tip" paid to the validator to incentivize transaction inclusion.
    - **`gwei(150)`**: The helper function is used to create a `BigInt` representing 150 Gwei for an accurate comparison.

### How to Run ([Dry-Run Mode](../../docs/src/operations/cli.md#dry-run-mode))

To test this monitor against historical blocks, use the `dry-run` command with the `--config-dir` argument pointing to this example's configuration:

```bash
cargo run --release -- dry-run  --from 23315052 --to 23315052 --config-dir examples/8_high_priority_fee/
```

Run with Docker image from GHCR:

```bash
docker run --rm \
  --env-file .env \
  -v "$(pwd)/examples/8_high_priority_fee:/app/configs:ro" \
  -v "$(pwd)/abis:/app/abis:ro" \
  ghcr.io/isserge/argus-rs:latest \
  dry-run --from 23315052 --to 23315052 --config-dir /app/configs
```

Replace `23315052` with any Ethereum block number to test against.

Run with `debug` logs:

```bash
RUST_LOG=debug cargo run --release -- dry-run --from 23315052 --to 23315052 --config-dir examples/8_high_priority_fee/
```

Replace `23315052` and `23315052` with any Ethereum block numbers to test against.


#### Expected Output

As blocks within the specified range are processed, you should see notifications printed directly to the console when transactions with high priority fees are detected.

Once processing is complete, you should see a summary report like this:

```
Dry Run Report
==============

Summary
-------
- Blocks Processed: 23315052 to 23315054 (3 blocks)
- Total Matches Found: 1

Matches by Monitor
------------------
- "High Priority Fee Alert": 1

Notifications Dispatched
------------------------
- "Stdout High Priority Fee": 1
```


### How to Run (Default Mode)

Once you have verified your monitor works against historical data in `dry-run` mode, you can start it in default (live monitoring) mode. In this mode, the monitor will continuously poll for new blocks and dispatch actual notifications via the configured action when a match is found.

```bash
cargo run --release -- run --config-dir examples/8_high_priority_fee/
```

Using Docker image from GHCR:

```bash
# First, create a data directory for this example
mkdir -p examples/8_high_priority_fee/data

# Run the container in detached mode
docker run --rm -d \
  --name argus_example_8 \
  --env-file .env \
  -v "$(pwd)/examples/8_high_priority_fee:/app/configs:ro" \
  -v "$(pwd)/abis:/app/abis:ro" \
  -v "$(pwd)/examples/8_high_priority_fee/data:/app/data" \
  ghcr.io/isserge/argus-rs:latest \
  run --config-dir /app/configs
```
