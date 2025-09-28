# 6. Notifier with Aggregation Policy

This example demonstrates how to use `aggregation` policy for notifiers as well as `sum` and `avg` filters in templates to aggregate values from multiple monitor matches. It uses the `stdout` notifier for simple console output.

### Configuration Files

- [`app.yaml`](../../docs/src/user_guide/config_app.md): Basic application configuration.
- [`monitors.yaml`](../../docs/src/user_guide/config_monitors.md): Defines the "Aggregated WBTC Transfers" monitor.
- [`notifiers.yaml`](../../docs/src/user_guide/config_notifiers.md): Defines a stdout notifier that aggregates WBTC transfer values.

### Notifier Options

This example uses the stdout notifier which prints notifications directly to the console. This is ideal for:
- Local development and testing
- Debugging monitor configurations and aggregation behavior
- Dry-run scenarios

For production use, you can configure other notifiers like Slack, Discord, Telegram, or webhooks. See the [Notifier Configuration documentation](../../docs/src/user_guide/notifiers_yaml.md) for all available options.

### Monitor Configuration

The `monitors.yaml` file defines a monitor that triggers for any WBTC `Transfer` event with a value greater than 0.001 WBTC. For a complete reference on monitor configuration, see the [Monitor Configuration documentation](../../docs/src/user_guide/config_monitors.md).

```yaml
monitors:
  - name: 'Aggregated WBTC Transfers'
    network: 'ethereum'
    address: '0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599'
    abi: 'wbtc'
    filter_script: |
      log.name == "Transfer" && log.params.value > wbtc(0.001)
    notifiers:
      - 'Stdout Aggregated WBTC Transfers'
```

### Notifier Configuration

The `notifiers.yaml` file defines a stdout notifier that uses the `sum` and `avg` filters to aggregate the values of the detected WBTC transfers, and an `aggregation` policy to group notifications within a time window. For a complete reference on notifier configuration, including policies and templating, see the [Notifier Configuration documentation](../../docs/src/user_guide/config_notifiers.md) and [Notifier Templating documentation](../../docs/src/user_guide/notifier_templating.md).

```yaml
notifiers:
  - name: "Stdout Aggregated WBTC Transfers"
    stdout:
      message:
        title: "Aggregated WBTC Transfers"
        body: |
          This template will not be used directly, as we are using an aggregation policy.
    policy:
      # The `aggregation` policy collects all matches that occur within a time
      # window and sends a single, consolidated notification.
      aggregation:
        # The duration of the aggregation window in seconds.
        # All matches for a given monitor within this period will be grouped.
        window_secs: 300 # 5 minutes
        # The template for the aggregated notification.
        # This template has access to a `matches` array, which contains all the
        # `MonitorMatch` objects collected during the window.
        template:
          title: "Event Summary for {{ monitor_name }}"
          # You can iterate over the `matches` or use filters like `length`.
          body: |
            Detected {{ matches | length }} WBTC transfers by monitor {{ monitor_name }}.
            Total value: {{ matches | map(attribute='log.params.value') | sum | wbtc }} WBTC
            Average value: {{ matches | map(attribute='log.params.value') | avg | wbtc }} WBTC
```

-   **`policy.aggregation`**: For more details on aggregation policies, see the [Notifier Configuration documentation](../../docs/src/user_guide/config_notifiers.md#aggregation-policy).
    -   `time_window_secs`: The duration of the time window in seconds during which notifications will be aggregated.

### How to Run ([Dry-Run Mode](../../docs/src/operations/cli.md#dry-run-mode))

To test this monitor against historical blocks, use the `dry-run` command with the `--config-dir` argument pointing to this example's configuration:

```bash
cargo run --release -- dry-run --from 23289380 --to 23289383 --config-dir examples/6_notifier_with_aggregation_policy/
```

Run with `debug` logs:

```bash
RUST_LOG=debug cargo run --release -- dry-run --from 23289380 --to 23289383 --config-dir examples/6_notifier_with_aggregation_policy/
```

Run with Docker image from GHCR:

```bash
docker run --rm \
  --env-file .env \
  -v "$(pwd)/examples/6_notifier_with_aggregation_policy:/app/configs:ro" \
  -v "$(pwd)/abis:/app/abis:ro" \
  ghcr.io/isserge/argus-rs:latest \
  dry-run --from 23289380 --to 23289383 --config-dir /app/configs
```

Replace `23289380` and `23289383` with any Ethereum block numbers to test against.

#### Expected Output

As blocks within the specified range are processed, you should see notifications printed directly to the console with aggregated values. The stdout notifier will display the aggregated notification when the time window expires.

Once processing is complete, you should see a summary report like this:

```
Dry Run Report
==============

Summary
-------
- Blocks Processed: 23289380 to 23289383 (4 blocks)
- Total Matches Found: 4

Matches by Monitor
------------------
- "Aggregated WBTC Transfers": 4

Notifications Dispatched
------------------------
- "Stdout Aggregated WBTC Transfers": 1
```

Note that with the aggregation policy, even though 4 matches were found, only 1 notification was dispatched because the matches were aggregated within the configured time window (300 seconds).

### How to Run (Default Mode)

Once you have verified your monitor works against historical data in `dry-run` mode, you can start it in default (live monitoring) mode. In this mode, the monitor will continuously poll for new blocks and dispatch actual notifications via the configured notifier when a match is found.

```bash
cargo run --release -- run --config-dir examples/6_notifier_with_aggregation_policy/
```

Using Docker image from GHCR:

```bash
# First, create a data directory for this example
mkdir -p examples/6_notifier_with_aggregation_policy/data

# Run the container in detached mode
docker run --rm -d \
  --name argus_example_6 \
  --env-file .env \
  -v "$(pwd)/examples/6_notifier_with_aggregation_policy:/app/configs:ro" \
  -v "$(pwd)/abis:/app/abis:ro" \
  -v "$(pwd)/examples/6_notifier_with_aggregation_policy/data:/app/data" \
  ghcr.io/isserge/argus-rs:latest \
  run --config-dir /app/configs
```
