# 9. Admin Function Call Monitor

This example demonstrates how to monitor for calls to a specific function on a contract using Argus's `decode_calldata` feature. This is the recommended approach for monitoring critical or administrative functions, as it is more robust and readable than manually checking function selectors. It uses the `stdout` notifier for simple console output.

This example uses a real-world event: the execution of an Aave governance proposal.

### Configuration Files

- [`app.yaml`](../../docs/src/user_guide/config_app.md): Basic application configuration, pointing to public RPC endpoints.
- [`monitors.yaml`](../../docs/src/user_guide/config_monitors.md): Defines the "Admin Function Call (Aave Governance V2)" monitor.
- [`notifiers.yaml`](../../docs/src/user_guide/config_notifiers.md): Defines the stdout notifier for console output.

### Notifier Options

This example uses the stdout notifier which prints notifications directly to the console. This is ideal for:
- Local development and testing
- Debugging monitor configurations and admin function detection
- Dry-run scenarios

For production use, you can configure other notifiers like Slack, Discord, Telegram, or webhooks. See the [Notifier Configuration documentation](../../docs/src/user_guide/notifiers_yaml.md) for all available options.

### Monitor Configuration

The `monitors.yaml` file defines a monitor that triggers when it detects a transaction calling the `execute()` function on the Aave Governance V2 contract.

```yaml
monitors:
  - name: "Admin Function Call (Aave Governance V2)"
    network: "ethereum"
    address: "0xEC568fffba86c094cf06b22134B23074DFE2252c"
    abi: "aave_governance_v2"
    filter_script: |
      decoded_call.name == "execute"
    notifiers:
      - "Stdout Admin"
```

- **`address`**: The specific contract to monitor.
- **`abi`**: The name of the ABI file (without the `.json` extension) located in the `abis/` directory. This is required for decoding.
- **`filter_script`**: This [Rhai script](../../docs/src/user_guide/rhai_scripts.md) inspects the `decoded_call` object.
- **`decoded_call.name == "execute"`**: This check is performed on the decoded function name.

### How to Run ([Dry-Run Mode](../../docs/src/operations/cli.md#dry-run-mode))

To test this monitor against historical blocks, use the `dry-run` command with the `--config-dir` argument pointing to this example's configuration:

```bash
cargo run --release -- dry-run --from 18864956 --to 18864956 --config-dir examples/9_admin_function_call/
```

Run with `debug` logs:

```bash
RUST_LOG=debug cargo run --release -- dry-run --from 18864956 --to 18864956 --config-dir examples/9_admin_function_call/
```

Run with Docker image from GHCR:

```bash
docker run --rm \
  --env-file .env \
  -v "$(pwd)/examples/9_admin_function_call:/app/configs:ro" \
  -v "$(pwd)/abis:/app/abis:ro" \
  ghcr.io/isserge/argus-rs:latest \
  dry-run --from 18864956 --to 18864956 --config-dir /app/configs
```

Replace `18864956` with any Ethereum block number to test against.


#### Expected Output

As blocks within the specified range are processed, you should see notifications printed directly to the console when admin function calls are detected.

Once processing is complete, you should see a summary report like this:

```
Dry Run Report
==============

Summary
-------
- Blocks Processed: 18864956 to 18864958 (3 blocks)
- Total Matches Found: 1

Matches by Monitor
------------------
- "Admin Function Call (Aave Governance V2)": 1

Notifications Dispatched
------------------------
- "Stdout Admin": 1
```


### How to Run (Default Mode)

Once you have verified your monitor works against historical data in `dry-run` mode, you can start it in default (live monitoring) mode. In this mode, the monitor will continuously poll for new blocks and dispatch actual notifications via the configured notifier when a match is found.

```bash
cargo run --release -- run --config-dir examples/9_admin_function_call/
```

Using Docker image from GHCR:

```bash
# First, create a data directory for this example
mkdir -p examples/9_admin_function_call/data

# Run the container in detached mode
docker run --rm -d \
  --name argus_example_9 \
  --env-file .env \
  -v "$(pwd)/examples/9_admin_function_call:/app/configs:ro" \
  -v "$(pwd)/abis:/app/abis:ro" \
  -v "$(pwd)/examples/9_admin_function_call/data:/app/data" \
  ghcr.io/isserge/argus-rs:latest \
  run --config-dir /app/configs
```

