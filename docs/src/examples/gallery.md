# Example Gallery

This section contains a gallery of complete, working examples that you can use as a starting point for your own monitors. Each example includes all necessary configuration files and a detailed explanation in its `README.md`.

The source for all examples can be found in the [`/examples` directory of the repository](https://github.com/isSerge/argus-rs/tree/main/examples).

---

### [1. Basic ETH Transfer Monitor](https://github.com/isSerge/argus-rs/tree/main/examples/1_basic_eth_transfer/README.md)

Monitors for native ETH transfers greater than a specific value. A great starting point for understanding transaction-based filtering.

**Features Demonstrated:** `tx.value`, `ether()` helper, basic notifier.

---

### [2. Large USDC Transfer Monitor](https://github.com/isSerge/argus-rs/tree/main/examples/2_large_usdc_transfer/README.md)

Monitors for `Transfer` events from a specific ERC20 contract (USDC) above a certain amount. Introduces event-based filtering.

**Features Demonstrated:** `log.name`, `log.params`, `address` and `abi` fields, `usdc()` helper.

---

### [3. WETH Deposit Monitor](https://github.com/isSerge/argus-rs/tree/main/examples/3_weth_deposit/README.md)

Monitors for `Deposit` events from the WETH contract, combining event and transaction data in the filter.

**Features Demonstrated:** Combining `log.*` and `tx.*` variables.

---

### [4. All ERC20 Transfers for a Wallet](https://github.com/isSerge/argus-rs/tree/main/examples/4_all_erc20_transfers_for_eoa/README.md)

Demonstrates a powerful global log monitor (`address: 'all'`) to catch all `Transfer` events involving a specific wallet, regardless of the token.

**Features Demonstrated:** Global log monitoring.

---

### [5. Notifier with Throttling Policy](https://github.com/isSerge/argus-rs/tree/main/examples/5_notifier_with_throttle_policy/README.md)

Shows how to configure a notifier with a `throttle` policy to limit the rate of notifications and prevent alert fatigue.

**Features Demonstrated:** Notifier policies.
