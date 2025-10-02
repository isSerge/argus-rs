# Example Gallery

This section contains a gallery of complete, working examples that you can use as a starting point for your own monitors. Each example includes all necessary configuration files and a detailed explanation in its `README.md`.

The source for all examples can be found in the [`/examples` directory of the repository](https://github.com/isSerge/argus-rs/tree/main/examples).

---

### [1. Basic ETH Transfer Monitor](https://github.com/isSerge/argus-rs/tree/main/examples/1_basic_eth_transfer/README.md)

Monitors for native ETH transfers greater than a specific value. A great starting point for understanding transaction-based filtering.

**Features Demonstrated:** `tx.value`, `ether()` helper, basic action.

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

### [5. Action with Throttling Policy](https://github.com/isSerge/argus-rs/tree/main/examples/5_action_with_throttle_policy/README.md)

Shows how to configure a action with a `throttle` policy to limit the rate of notifications and prevent alert fatigue.

**Features Demonstrated:** Action policies.

---

### [6. Action with Aggregation Policy](https://github.com/isSerge/argus-rs/tree/main/examples/6_action_with_aggregation_policy/README.md)

Demonstrates how to use `aggregation` policy for actions as well as `sum` and `avg` filters in templates to aggregate values from multiple monitor matches.

**Features Demonstrated:** Aggregation policy, `map`, `sum`, `avg` filters.

---

### [7. Address Watchlist Monitor](https://github.com/isSerge/argus-rs/tree/main/examples/7_address_watchlist_monitor/README.md)

Shows how to use a Rhai array as a watchlist to get notifications for any transaction involving a specific set of addresses.

**Features Demonstrated:** Rhai arrays, `let` variables, `in` operator.

### [8.High Priority Fee](https://github.com/isSerge/argus-rs/tree/main/examples/8_high_priority_fee/README.md)

Demonstrates how to monitor for transactions with unusually high priority fees, which can be an indicator of MEV (Maximal Extractable Value) activity, front-running, or other urgent on-chain actions.

### [9. Admin Function Call Monitor](https://github.com/isSerge/argus-rs/tree/main/examples/9_admin_function_call/README.md)

Demonstrates how to monitor for calls to a specific function on a contract using `decode_calldata` feature. This is the recommended approach for monitoring critical or administrative functions

**Features Demonstrated:** `decode_calldata`.

---

### [10. Action with Kafka Publisher](https://github.com/isSerge/argus-rs/tree/main/examples/10_action_with_kafka_publisher/README.md)

Demonstrates how to configure a `kafka` action to send notifications to a Kafka topic, ideal for integrating with data streaming platforms.

**Features Demonstrated:** `kafka` action.

---

### [11. Action with RabbitMQ Publisher](https://github.com/isSerge/argus-rs/tree/main/examples/11_action_with_rabbitmq_publisher/README.md)

Shows how to set up a `rabbitmq` action to publish notifications to a RabbitMQ exchange for integration with message queue-based systems.

**Features Demonstrated:** `rabbitmq` action.


