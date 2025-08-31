# Examples

This directory contains various examples demonstrating how to configure and use
Argus EVM Monitor.

## List of Examples

- [1. Basic ETH Transfer Monitor](./1_basic_eth_transfer/README.md): This
  example sets up a monitor that triggers when a transaction with a value
  greater than 10 ETH is detected on the Ethereum mainnet.
- [2. Large USDC Transfer Monitor](./2_large_usdc_transfer/README.md): This
  example sets up a monitor that triggers when a `Transfer` event with a value
  greater than 1,000,000 USDC is detected from the USDC contract on the Ethereum
  mainnet.
- [3. WETH Deposit](./3_weth_deposit/README.md): This example sets up a monitor that triggers when a `Deposit` event is detected from the WETH contract on the Ethereum mainnet.

---

## Have a great use case you'd like to share?

If you have a use case that you think would be valuable to add here as an
example, feel free to
[submit an issue](https://github.com/isSerge/argus-rs/issues/new/choose) with
your suggestion!
