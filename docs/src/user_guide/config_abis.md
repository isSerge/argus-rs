# ABI Management

To decode event logs from smart contracts, Argus needs access to the contract's Application Binary Interface (ABI). This section explains how to provide and manage ABIs for your monitors.

## What is an ABI?

An ABI (Application Binary Interface) is a JSON file that describes a smart contract's public interface, including its functions and events. Argus uses the ABI to parse the raw log data emitted by a contract into a human-readable format that you can access in your Rhai scripts (e.g., `log.name`, `log.params`).

You only need to provide an ABI if your monitor needs to inspect event logs (i.e., if your `filter_script` accesses the `log` variable).

## How Argus Finds ABIs

1.  **ABI Directory**: In your [`app.yaml`](./app_yaml.md), you specify the path to your ABI directory using the `abi_config_path` parameter (default is `abis/`).

2.  **JSON Files**: Argus expects to find ABI files in this directory with a `.json` extension.

3.  **Naming Convention**: When you define a monitor in [`monitors.yaml`](./monitors_yaml.md) that needs an ABI, you set the `abi` field to the **name of the JSON file without the extension**.

## Example Workflow

Let's say you want to monitor `Transfer` events from the USDC contract.

1.  **Get the ABI**: First, you need the ABI for the USDC contract. You can usually get this from the contract's page on a block explorer like Etherscan. Save it as a JSON file.

    For proxy contracts (like USDC), you will need the ABI of the **implementation** contract, not the proxy itself.

2.  **Save the ABI File**: Save the ABI file into your configured ABI directory. For this example, you would save it as `abis/usdc.json`.

3.  **Reference in Monitor**: In your [`monitors.yaml`](./monitors_yaml.md), reference the ABI by its filename (without the `.json` extension).

    ```yaml
    # monitors.yaml
    monitors:
      - name: "Large USDC Transfers"
        network: "ethereum"
        address: "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48"
        # This tells Argus to load `abis/usdc.json` to decode logs for this monitor.
        abi: "usdc"
        filter_script: |
          log.name == "Transfer" && log.params.value > usdc(1_000_000)
        notifiers:
          - "my-webhook"
    ```

When Argus starts, it will load all the `.json` files from the `abi_config_path` directory. When the "Large USDC Transfers" monitor runs, it will use the pre-loaded `usdc` ABI to decode any event logs from the specified contract address.
