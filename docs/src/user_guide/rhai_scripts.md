# Writing Rhai Scripts

Argus is using the [Rhai](https://rhai.rs) scripting language for its filtering logic. Each monitor you define has a `filter_script` that determines whether a given transaction or log should trigger a notification.

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

## Data Types, Conversions, and Custom Filters in Rhai

When working with blockchain data in Rhai scripts, it's crucial to understand how data types are handled, especially for large numerical values, and how to use conversion functions for accurate comparisons and calculations.

### Handling Large Numbers (BigInts)

EVM-compatible blockchains often deal with very large numbers (e.g., token amounts in Wei, Gwei, or other base units) that exceed the capacity of standard 64-bit integers. In Argus's Rhai environment, these large numbers are typically represented as `BigInt` strings.

To perform mathematical comparisons or operations on these `BigInt` strings, you **must** use the provided conversion helper functions. These functions convert the `BigInt` string into a comparable decimal representation.

### Conversion Helper Functions

Argus provides several built-in helper functions to facilitate these conversions:

*   **`ether(amount)`**: Converts a decimal `amount` (e.g., `10.5`) into its equivalent `BigInt` string in Wei (18 decimal places).
    ```rhai
    tx.value > ether(10) // Checks if tx.value (BigInt string) is greater than 10 ETH
    ```

*   **`gwei(amount)`**: Converts a decimal `amount` into its equivalent `BigInt` string in Gwei (9 decimal places).
    ```rhai
    tx.gas_price > gwei(50) // Checks if gas_price (BigInt string) is greater than 50 Gwei
    ```

*   **`usdc(amount)`**: Converts a decimal `amount` into its equivalent `BigInt` string for USDC (6 decimal places).
    ```rhai
    log.params.value > usdc(1_000_000) // Checks if log.params.value (BigInt string) is greater than 1,000,000 USDC
    ```

*   **`wbtc(amount)`**: Converts a decimal `amount` into its equivalent `BigInt` string for WBTC (8 decimal places).
    ```rhai
    log.params.value > wbtc(0.5) // Checks if log.params.value (BigInt string) is greater than 0.5 WBTC
    ```

*   **`decimals(amount, num_decimals)`**: A generic function to convert a decimal `amount` into a `BigInt` string with a specified number of decimal places.
    ```rhai
    log.params.tokenAmount > decimals(100, 0) // Checks if tokenAmount (BigInt string) is greater than 100 with 0 decimals
    ```

**Important**: Always use these conversion functions when comparing or performing arithmetic with `tx.value`, `log.params.value`, or other `BigInt` string representations of token amounts to ensure correct logic. If you are unsure when to apply a conversion, use the [`dry-run` feature](../operations/cli.md#dry-run-mode) to verify your script works as expected.

## The Data Context

Within your script, you have access to a rich set of data about the on-chain event. The data available depends on the type of monitor you have configured.

-   **For all monitors**, you have access to the `tx` object, which contains details about the transaction.
-   **For monitors with an `address` and `abi`**, you also have access to the `log` object, which contains the decoded event log data.

For a detailed breakdown of the available data, see the following pages:

-   [**Data Context (`tx` & `log`)**](./rhai_context.md)
-   [**Helper Functions (`ether`, `usdc`, etc.)**](./rhai_helpers.md)

## Safe Property Access (`?.`)

In many cases, context variables may be `()` (Rhai's version of `null`). For example, `decoded_call` is null if a transaction's input data cannot be decoded by a monitor's ABI.

To simplify scripts and prevent runtime errors, use the Elvis operator (`?.`) for safe property access. This is similar to the optional chaining operator in JavaScript.

### Example: `decoded_call` Check

```rhai
// This script safely checks the function name, even if decoded_call is null.
decoded_call?.name == "execute"
```

If `decoded_call` is `()`, the expression `decoded_call?.name` will also evaluate to `()` instead of erroring. The final comparison, `() == "execute"`, safely resolves to `false`. This feature makes your scripts cleaner, safer, and more intuitive.

## Scripting Best Practices

1.  **Keep it Simple**: Your script is executed for every relevant transaction or log. Keep it as simple and efficient as possible.
2.  **Be Specific**: The more specific your filter, the fewer false positives you'll get.
3.  **Use Helpers**: Use the provided helper functions (`ether`, `usdc`, `gwei`, `decimals`) to handle token amounts. This makes your scripts more readable and less error-prone.
4.  **Test with `dry-run`**: Before deploying a new or modified monitor, use the `dry-run` CLI command to test your script against historical data. This will help you verify that it's working as expected.

For more practical examples of Rhai scripts, refer to the [Example Gallery](../examples/gallery.md).
