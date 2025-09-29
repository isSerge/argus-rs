# Monitor Configuration (monitors.yaml)

Monitors are the core of Argus, defining what events to watch for on the blockchain and what actions to take when those events occur. This document explains how to configure your `monitors.yaml` file.

## Basic Structure

A `monitors.yaml` file contains a list of monitor definitions. Each monitor has a `name`, `network`, `filter_script`, and a list of `actions`.

```yaml
monitors:
  - name: "Large ETH Transfers"
    network: "ethereum"
    filter_script: |
      tx.value > ether(10)
    actions:
      - "Telegram Large ETH Transfers"
```

## Monitor Fields

*   **`name`** (string, required): A unique, human-readable name for the monitor.
*   **`network`** (string, required): The blockchain network this monitor should observe (e.g., "ethereum", "sepolia", "arbitrum"). This must correspond to a network configured in your [`app.yaml`](./app_yaml.md).
*   **`address`** (string, optional): The contract address to monitor. If omitted, the monitor will process all **transactions** on the specified `network` (useful for native token transfers). Set to `"all"` to create a global **log** monitor that processes all logs on the network (requires an `abi`). See [Example 1: Basic ETH Transfer Monitor](../examples/1_basic_eth_transfer/README.md) for an example without an address, and [Example 4: All ERC20 Transfers for a Wallet](../examples/4_all_erc20_transfers_for_eoa/README.md) for an example of global log monitoring.
*   **`abi`** (string, optional): The name of the ABI (Application Binary Interface) to use for decoding contract events. This name should correspond to a `.json` file (without the `.json` extension) located in the `abis/` directory (or the directory configured for ABIs in [`app.yaml`](./app_yaml.md)). Required if `filter_script` accesses `log` data. See [ABI Management](./config_abis.md) for more details and [Example 2: Large USDC Transfer Monitor](../examples/2_large_usdc_transfer/README.md) for an example.
*   **`actions`** (list of strings, required): A list of names of actions (defined in [`actions.yaml`](./actions_yaml.md)) that should be triggered when this monitor's `filter_script` returns `true`.

## Monitor Validation

Argus performs several validation checks on your monitor configurations at startup to ensure they are correctly defined and can operate as expected. If any validation fails, the application will not start and will report a detailed error.

Here are the key validation rules:

*   **Network Mismatch**: The `network` specified in a monitor must exactly match the `network_id` configured in your [`app.yaml`](./app_yaml.md).

*   **Unknown Action**: Every action name listed in a monitor's `actions` field must correspond to a `name` defined in your [`actions.yaml`](./actions_yaml.md) file.

*   **Invalid Address**: If an `address` is provided, it must be a valid hexadecimal Ethereum address (e.g., `0x...`) or the special string `"all"` for global log monitoring.

*   **Script Compilation**: The `filter_script` must be valid Rhai code that compiles without errors.

*   **Script Static Analysis**: Argus analyzes your Rhai script to prevent common logical errors before they can cause issues at runtime. This includes:
    *   **Log Access without ABI**: If your script accesses the `log` variable (e.g., `log.name`), the monitor *must* have an `abi` field defined.
    *   **ABI Requirement**: If an `abi` is specified, the corresponding ABI file must exist in the configured `abi_config_path` and be a valid JSON ABI.
    *   **Return Type**: The script must evaluate to a boolean (`true` or `false`). Argus will reject scripts that return other types.
    *   **Invalid Field Access**: The script is checked for invalid field access (e.g., `tx.foobar`, `log.params.nonexistent`). The validator uses the provided ABI to ensure that `log.params` access is valid.

Using the [`dry-run` CLI command](../operations/cli.md#dry-run) is highly recommended to test your monitor configurations and scripts against historical data, which can help catch validation issues early.

## Examples

For more detailed examples of monitor configurations, refer to the [Example Gallery](../examples/gallery.md).
