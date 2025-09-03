# Rhai Helper Functions

To make writing filter scripts easier and less error-prone, Argus provides a set of built-in helper functions. These functions are primarily designed to simplify the handling of large numbers and different token denominations.

All numeric values from the EVM, such as transaction values and token amounts, are represented as `BigInt` types in Rhai to avoid precision loss. These helpers allow you to work with them in a more natural way.

## Denomination Helpers

These functions convert a human-readable number into its wei-equivalent `BigInt` based on the token's decimal places.

### `ether(value)`

Converts a number into a `BigInt` with 18 decimal places. Useful for ETH and other 18-decimal tokens (e.g., WETH).

-   `value`: An integer or float.

**Example:**
`ether(10)` is equivalent to `bigint("10000000000000000000")`.

```rhai
// Check if more than 10 ETH was transferred
tx.value > ether(10)
```

### `gwei(value)`

Converts a number into a `BigInt` with 9 decimal places.

-   `value`: An integer or float.

**Example:**
`gwei(20)` is equivalent to `bigint("20000000000")`.

```rhai
// Check if the gas price is over 20 gwei
tx.gas_price > gwei(20)
```

### `usdc(value)`

Converts a number into a `BigInt` with 6 decimal places. Specifically for USDC and other 6-decimal stablecoins.

-   `value`: An integer or float.

**Example:**
`usdc(1_000_000)` is equivalent to `bigint("1000000000000")`.

```rhai
// Check for a USDC transfer of over 1,000,000
log.params.value > usdc(1_000_000)
```

## Generic Decimal Helper

### `decimals(value, places)`

This is a generic version of the denomination helpers that allows you to specify the number of decimal places. This is useful for working with any ERC20 token.

-   `value`: An integer or float.
-   `places`: The number of decimal places for the token (integer).

**Example:**
If you are monitoring a token with 8 decimal places, you can use `decimals` to correctly scale the value.

```rhai
// For a token with 8 decimal places, check for a transfer of over 5,000
log.params.value > decimals(5000, 8)
```

## BigInt Helper

### `bigint(value)`

Converts a string into a `BigInt`. This is useful for representing very large numbers that might not fit in a standard integer type.

-   `value`: A string representing a large integer.

**Example:**

```rhai
// A very large number
let threshold = bigint("5000000000000000000000");
tx.value > threshold
```
