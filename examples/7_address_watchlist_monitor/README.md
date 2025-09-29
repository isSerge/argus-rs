# 7. Address Watchlist Monitor

This example demonstrates a powerful Rhai script feature: using an array as a watchlist to monitor for any activity involving a specific set of addresses, in this case we're using Ethereum Foundation addresses. It uses the `stdout` action for simple console output.

### Configuration Files

- [`app.yaml`](../../docs/src/user_guide/config_app.md): Basic application configuration, pointing to public RPC endpoints.
- [`monitors.yaml`](../../docs/src/user_guide/config_monitors.md): Defines the "Address Watchlist" monitor.
- [`actions.yaml`](../../docs/src/user_guide/config_actions.md): Defines the stdout action for console output.

### Action Options

This example uses the stdout action which prints notifications directly to the console. This is ideal for:
- Local development and testing
- Debugging monitor configurations and watchlist behavior
- Dry-run scenarios

For production use, you can configure other actions like Slack, Discord, Telegram, or webhooks. See the [Action Configuration documentation](../../docs/src/user_guide/actions_yaml.md) for all available options.

### Monitor Configuration

The `monitors.yaml` file defines a monitor that triggers for any transaction where the `from` or `to` address is in the predefined `watchlist`.

```yaml
monitors:
  - name: "Address Watchlist"
    network: "ethereum"
    filter_script: |
      let watchlist = [
          "0x5eD8Cee6b63b1c6AFce3AD7c92f4fD7E1B8fAd9F", // EF1
          "0xc06145782F31030dB1C40B203bE6B0fD53410B6d", // EF2
          "0x9fC3dc011b461664c835F2527fffb1169b3C213e", // EF: DeFi Multisig
          "0xde0B295669a9FD93d5F28D9Ec85E40f4cb697BAe"  // EthDev
      ];

      tx.from in watchlist || tx.to in watchlist
    actions:
      - "Telegram Watchlist"
```

- **`filter_script`**: This [Rhai script](../../docs/src/user_guide/rhai_scripts.md) showcases two key features:
    - **`let watchlist = [...]`**: Declares an array of addresses to monitor.
    - **`tx.from in watchlist`**: Uses the `in` operator to check if the transaction's sender or recipient is present in the `watchlist` array.

### How to Run ([Dry-Run Mode](../../docs/src/operations/cli.md#dry-run-mode))

To test this monitor against historical blocks, use the `dry-run` command with the `--config-dir` argument pointing to this example's configuration:

```bash
cargo run --release -- dry-run --from 22895951 --to 22895952 --config-dir examples/7_address_watchlist_monitor/
```

Run with `debug` logs:

```bash
RUST_LOG=debug cargo run --release -- dry-run --from 22895951 --to 22895952 --config-dir examples/7_address_watchlist_monitor/
```

Run with Docker image from GHCR:

```bash
docker run --rm \
  --env-file .env \
  -v "$(pwd)/examples/7_address_watchlist_monitor:/app/configs:ro" \
  -v "$(pwd)/abis:/app/abis:ro" \
  ghcr.io/isserge/argus-rs:latest \
  dry-run --from 22895951 --to 22895952 --config-dir /app/configs
```

Replace `22895951` and `22895952` with any Ethereum block numbers to test against.

#### Expected Output

As blocks within the specified range are processed, you should see notifications printed directly to the console when transactions involve the watched addresses.

Once processing is complete, you should see a summary report like this:

```
Dry Run Report
==============

Summary
-------
- Blocks Processed: 22895951 to 22895952 (2 blocks)
- Total Matches Found: 1

Matches by Monitor
------------------
- "Address Watchlist": 1

Notifications Dispatched
------------------------
- "Stdout Watchlist": 1
```

### How to Run (Default Mode)

Once you have verified your monitor works against historical data in `dry-run` mode, you can start it in default (live monitoring) mode. In this mode, the monitor will continuously poll for new blocks and dispatch actual notifications via the configured action when a match is found.

```bash
cargo run --release -- run --config-dir examples/7_address_watchlist_monitor/
```

Using Docker image from GHCR:

```bash
# First, create a data directory for this example
mkdir -p examples/7_address_watchlist_monitor/data

# Run the container in detached mode
docker run --rm -d \
  --name argus_example_7 \
  --env-file .env \
  -v "$(pwd)/examples/7_address_watchlist_monitor:/app/configs:ro" \
  -v "$(pwd)/abis:/app/abis:ro" \
  -v "$(pwd)/examples/7_address_watchlist_monitor/data:/app/data" \
  ghcr.io/isserge/argus-rs:latest \
  run --config-dir /app/configs
```
