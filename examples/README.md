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
- [4. All ERC20 Transfers Monitor for Specific EOA](./4_all_erc20_transfers_for_eoa/README.md): This example sets up a global log monitor that triggers for any `Transfer` event from any ERC20-compliant contract on the Ethereum mainnet involving specific EOA.
- [5. Monitor with Throttling Policy](./5_notifier_with_throttle_policy/README.md): This example demonstrates how to configure a notifier with a `throttle` policy to limit the rate of notifications.
- [6. Notifier with Aggregation Policy](./6_notifier_with_aggregation_policy/README.md): This example demonstrates how to use the `sum` and `avg` filters in notifier templates to aggregate values from multiple monitor matches.

---

## Have a great use case you'd like to share?

If you have a use case that you think would be valuable to add here as an
example, feel free to
[submit an issue](https://github.com/isSerge/argus-rs/issues/new/choose) with
your suggestion!
