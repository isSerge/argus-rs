# 5. Monitor with Throttling Policy

This example demonstrates how to configure a action with a `throttle` policy
to limit the rate of notifications and prevent alert fatigue. It uses the `stdout`
action for simple console output.

### Configuration Files

- [`app.yaml`](../../docs/src/user_guide/config_app.md): Basic application configuration.
- [`monitors.yaml`](../../docs/src/user_guide/config_monitors.md): Defines the "Large ETH Transfers with Throttling" monitor.
- [`actions.yaml`](../../docs/src/user_guide/config_actions.md): Defines a stdout action with a `throttle` policy.

### Action Options

This example uses the stdout action which prints notifications directly to the console. This is ideal for:
- Local development and testing
- Debugging monitor configurations and throttling behavior
- Dry-run scenarios

For production use, you can configure other actions like Slack, Discord, Telegram, or webhooks. See the [Action Configuration documentation](../../docs/src/user_guide/actions_yaml.md) for all available options.

### Monitor Configuration

The `monitors.yaml` file defines a monitor that triggers for any Ethereum
transaction with a value greater than 1 ETH. For a complete reference on monitor configuration, see the [Monitor Configuration documentation](../../docs/src/user_guide/config_monitors.md).

```yaml
monitors:
  - name: 'Large ETH Transfers with Throttling'
    network: 'ethereum'
    filter_script: |
      tx.value > ether(1)
    actions:
      - 'Throttled Stdout Action'
```

### Action Configuration

The `actions.yaml` file defines a stdout action with a `throttle`
policy. For a complete reference on action configuration, including policies, see the [Action Configuration documentation](../../docs/src/user_guide/actions_yaml.md).

```yaml
actions:
  - name: 'Throttled Stdout Action'
    stdout:
      message:
        title: 'Large ETH Transfer (Throttled)'
        body: |
          A transfer of over 1 ETH was detected by monitor {{ monitor_name }}.
          - From: `{{ tx.from }}`
          - To: `{{ tx.to }}`
          - Value: `{{ tx.value | ether }} ETH`
          [View on Etherscan](https://etherscan.io/tx/{{ tx.hash }})
    policy:
      throttle:
        # Send at most 5 notifications every 10 minutes
        max_count: 5
        time_window_secs: 600
```

-   **`policy.throttle`**: For more details on throttling policies, see the [Action Configuration documentation](../../docs/src/user_guide/config_actions.md#throttle-policy).
    -   `max_count`: The maximum number of notifications to send within the
        time window.
    -   `time_window_secs`: The duration of the time window in seconds.

### How to Run ([Dry-Run Mode](../../docs/src/operations/cli.md#dry-run-mode))

You can test this configuration using the `dry-run` command:

```bash
cargo run --release -- dry-run --from 23159290 --to 23159300 --config-dir examples/5_action_with_throttle_policy
```

Run with `debug` logs:

```bash
RUST_LOG=debug cargo run --release -- dry-run --from 23159290 --to 23159300 --config-dir examples/5_action_with_throttle_policy
```

Run with Docker image from GHCR:

```bash
docker run --rm \
  --env-file .env \
  -v "$(pwd)/examples/5_action_with_throttle_policy:/app/configs:ro" \
  -v "$(pwd)/abis:/app/abis:ro" \
  ghcr.io/isserge/argus-rs:latest \
  dry-run --from 23159290 --to 23159300 --config-dir /app/configs
```

Replace `23159290` and `23159300` with any Ethereum block numbers to test against.

#### Expected Output

As blocks within the specified range are processed, you should see **only 5 notifications** printed to the console (due to the throttle policy), even though more matches are found. Each notification will be formatted according to the message template:

```
Large ETH Transfer (Throttled)
A transfer of over 1 ETH was detected by monitor Large ETH Transfers with Throttling.
- From: `0x841FA51e9DCb53844720cDFdb13286554B4854eD`
- To: `0xa51a5FA9A309942f67Ad0872d8860A468EB91851`
- Value: `1.197082720736539353 ETH`
[View on Etherscan](https://etherscan.io/tx/0x3ab53e8efc91dbace37b6f390208e2dae4f9959415bcff61a92d8fad4fa133cc)
```

After `dry-run` processing is complete, you should see a summary report showing that while 45 matches were found, only 5 notifications were actually dispatched due to throttling:

```
Dry Run Report
==============

Summary
-------
- Blocks Processed: 23159290 to 23159300 (11 blocks)
- Total Matches Found: 45

Matches by Monitor
------------------
- "Large ETH Transfers with Throttling": 45

Notifications Dispatched
------------------------
- "Throttled Stdout Action": 5
```

This demonstrates the throttling policy in action: all 45 matches were detected, but only the first 5 triggered actual notifications within the 10-minute time window.

### How to Run (Default Mode)

Once you have verified your monitor works against historical data in `dry-run`
mode, you can start it in default (live monitoring) mode. In this mode, the
monitor will continuously poll for new blocks and dispatch actual notifications
via the configured action when a match is found.

```bash
cargo run --release -- run --config-dir examples/5_action_with_throttle_policy/
```

Using Docker image from GHCR:

```bash
# First, create a data directory for this example
mkdir -p examples/5_action_with_throttle_policy/data

# Run the container in detached mode
docker run --rm -d \
  --name argus_example_5 \
  --env-file .env \
  -v "$(pwd)/examples/5_action_with_throttle_policy:/app/configs:ro" \
  -v "$(pwd)/abis:/app/abis:ro" \
  -v "$(pwd)/examples/5_action_with_throttle_policy/data:/app/data" \
  ghcr.io/isserge/argus-rs:latest \
  run --config-dir /app/configs
```
