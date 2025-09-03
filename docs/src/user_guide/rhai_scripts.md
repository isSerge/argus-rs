# Writing Rhai Scripts

The core of Argus's flexibility comes from the use of the [Rhai](https://rhai.rs) scripting language for its filtering logic. Each monitor you define has a `filter_script` that determines whether a given transaction or log should trigger a notification.

## The `filter_script`

The `filter_script` is a short piece of Rhai code that must evaluate to a boolean (`true` or `false`).

-   If the script evaluates to `true`, the monitor is considered a "match," and a notification is sent to its configured notifiers.
-   If the script evaluates to `false`, no action is taken.

### Example: Simple ETH Transfer

This script triggers if a transaction's value is greater than 10 ETH.

```rhai
tx.value > ether(10)
```

### Example: Specific ERC20 Transfer Event

This script triggers if a log is a `Transfer` event and the `value` parameter of that event is greater than 1,000,000 USDC.

```rhai
log.name == "Transfer" && log.params.value > usdc(1_000_000)
```

## The Data Context

Within your script, you have access to a rich set of data about the on-chain event. The data available depends on the type of monitor you have configured.

-   **For all monitors**, you have access to the `tx` object, which contains details about the transaction.
-   **For monitors with an `address` and `abi`**, you also have access to the `log` object, which contains the decoded event log data.

For a detailed breakdown of the available data, see the following pages:

-   [**Data Context (`tx` & `log`)**](./rhai_context.md)
-   [**Helper Functions (`ether`, `usdc`, etc.)**](./rhai_helpers.md)

## Scripting Best Practices

1.  **Keep it Simple**: Your script is executed for every relevant transaction or log. Keep it as simple and efficient as possible.
2.  **Be Specific**: The more specific your filter, the fewer false positives you'll get.
3.  **Use Helpers**: Use the provided helper functions (`ether`, `usdc`, `gwei`, `decimals`) to handle token amounts. This makes your scripts more readable and less error-prone.
4.  **Test with `dry-run`**: Before deploying a new or modified monitor, use the `dry-run` CLI command to test your script against historical data. This will help you verify that it's working as expected.
