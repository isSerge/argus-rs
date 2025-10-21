# Action Templating

Argus allows you to customize the content of your notifications using templating. This enables dynamic messages that include details from the matched blockchain events.

## How Templating Works

When a monitor's `filter_script` matches an event, the relevant data (e.g., transaction details, log data) is passed to the configured actions. actions that support templating can then use this data to render a custom message.

Templates are typically defined within your [`actions.yaml`](./actions_yaml.md) file as part of the action's `message` configuration. The exact syntax and available variables will depend on the specific action type and the templating engine it uses (e.g., Handlebars-like syntax for basic fields, or more advanced templating for `body` fields).

## Available Data for Templating

All notification templates have access to the following top-level fields from the `MonitorMatch` object:

*   **`monitor_id`**: The unique ID of the monitor that triggered the match (integer).
*   **`monitor_name`**: The human-readable name of the monitor (string).
*   **`action_name`**: The name of the action handling this match (string).
*   **`block_number`**: The block number where the match occurred (integer).
*   **`transaction_hash`**: The hash of the transaction associated with the match (string).

Additionally, the following structured objects are always available:

*   **`tx`**: An object containing details about the transaction. This will be `null` if the match is log-based.
    *   `tx.from`: The sender address of the transaction (string).
    *   `tx.to`: The recipient address of the transaction (string, can be null for contract creations).
    *   `tx.hash`: The transaction hash (string).
    *   `tx.value`: The value transferred in the transaction (string, as a large number).
    *   `tx.gas_limit`: The gas limit for the transaction (integer).
    *   `tx.nonce`: The transaction nonce (integer).
    *   `tx.input`: The transaction input data (string).
    *   `tx.block_number`: The block number where the transaction was included (integer).
    *   `tx.transaction_index`: The index of the transaction within its block (integer).
    *   `tx.gas_price`: (Legacy transactions) The gas price (string, as a large number).
    *   `tx.max_fee_per_gas`: (EIP-1559 transactions) The maximum fee per gas (string, as a large number).
    *   `tx.max_priority_fee_per_gas`: (EIP-1559 transactions) The maximum priority fee per gas (string, as a large number).
    *   `tx.gas_used`: (From receipt) The gas used by the transaction (string, as a large number).
    *   `tx.status`: (From receipt) The transaction status (integer, 1 for success, 0 for failure).
    *   `tx.effective_gas_price`: (From receipt) The effective gas price (string, as a large number).

*   **`log`**: An object containing details about the log/event. This will be `null` if the match is transaction-based.
    *   `log.address`: The address of the contract that emitted the log (string).
    *   `log.log_index`: The index of the log within its block (integer).
    *   `log.name`: The name of the decoded event (string, e.g., "Transfer").
    *   `log.params`: A map of the event's decoded parameters (e.g., `log.params.from`, `log.params.to`, `log.params.value`).

*   **`decoded_call`**: An object containing details about decoded function call input data. This will be `null` if the transaction doesn't contain decodable function call data or if the monitor doesn't use an ABI.
    *   `decoded_call.name`: The name of the decoded function (string, e.g., "transfer", "approve").
    *   `decoded_call.params`: A map of the function's decoded parameters (e.g., `decoded_call.params._to`, `decoded_call.params._value`).

## Data Types, Conversions, and Custom Filters in Templates

When displaying blockchain data in Jinja2 templates, especially large numerical values, it's essential to use the provided custom filters for proper formatting and mathematical operations.

### Handling Large Numbers (BigInts)

Similar to Rhai scripts, large numerical values from the EVM (e.g., `tx.value`, `log.params.value`) are passed to templates as `BigInt` strings. While these raw string values will be displayed if no conversion is applied, it is highly recommended to use conversion filters for a more user-friendly and accurate representation, especially when performing calculations.

### Conversion and Utility Filters

Argus provides several custom Jinja2 filters to help you work with blockchain data:

*   **`ether`**: Converts a `BigInt` string (representing Wei) into its equivalent decimal value in Ether (18 decimal places).
    ```jinja
    {{ tx.value | ether }} ETH
    ```
    *Example*: If `tx.value` is `"1500000000000000000"`, the expression `{{ tx.value | ether }}` renders `1.5`, and the full template would output `1.5 ETH`.

*   **`gwei`**: Converts a `BigInt` string (representing Wei) into its equivalent decimal value in Gwei (9 decimal places).
    ```jinja
    {{ tx.gas_price | gwei }} Gwei
    ```
    *Example*: If `tx.gas_price` is `"20000000000"`, `{{ tx.gas_price | gwei }}` renders `20.0`, and the full template would output `20.0 Gwei`.

*   **`usdc`**: Converts a `BigInt` string into its equivalent decimal value for USDC (6 decimal places).
    ```jinja
    {{ log.params.value | usdc }} USDC
    ```
    *Example*: If `log.params.value` is `"50000000"`, `{{ log.params.value | usdc }}` renders `50.0`, and the full template would output `50.0 USDC`.

*   **`wbtc`**: Converts a `BigInt` string into its equivalent decimal value for WBTC (8 decimal places).
    ```jinja
    {{ log.params.value | wbtc }} WBTC
    ```
    *Example*: If `log.params.value` is `"100000000"`, `{{ log.params.value | wbtc }}` renders `1.0`, and the full template would output `1.0 WBTC`.

*   **`decimals(num_decimals)`**: A generic filter to convert a `BigInt` string into a decimal value with a specified number of decimal places.
    ```jinja
    {{ log.params.tokenAmount | decimals(18) }}
    ```
    *Example*: If `log.params.tokenAmount` is `"123450000000000000000"`, `{{ log.params.tokenAmount | decimals(18) }}` renders `123.45`.

*   **`map(attribute)`**: Extracts a specific `attribute` from each item in an array. This is particularly useful with aggregation policies to get a list of values for `sum` or `avg`.
    ```jinja
    {{ matches | map(attribute='log.params.value') }}
    ```

*   **`sum`**: Calculates the sum of a list of numerical values. Often used in conjunction with `map` and a conversion filter.
    ```jinja
    {{ matches | map(attribute='log.params.value') | sum | wbtc }} WBTC
    ```

*   **`avg`**: Calculates the average of a list of numerical values. Often used in conjunction with `map` and a conversion filter.
    ```jinja
    {{ matches | map(attribute='log.params.value') | avg | wbtc }} WBTC
    ```

**Important**: Always apply the appropriate conversion filter (e.g., `ether`, `usdc`, `wbtc`, `decimals`) to `BigInt` strings *before* performing mathematical operations like `sum` or `avg` to ensure accurate results. For example, `{{ matches | map(attribute='log.params.value') | sum | wbtc }}` correctly sums the values and then formats them as WBTC.

## Example: Generic Webhook Action

```yaml
actions:
  - name: "my-generic-webhook"
    webhook:
      url: "https://my-service.com/webhook-endpoint"
      method: "POST"
      message:
        title: "New Transaction Alert: {{ monitor_name }}"
        body: |
          A new match was detected for monitor {{ monitor_name }}.
          - **Block Number**: {{ block_number }}
          - **Transaction Hash**: {{ transaction_hash }}
          - **Contract Address**: {{ log.address }}
          - **Log Index**: {{ log.log_index }}
          - **Log Name**: {{ log.name }}
          - **Log Params**: {{ log.params }}
          {% if decoded_call %}
          - **Function Called**: {{ decoded_call.name }}
          - **Function Params**: {{ decoded_call.params }}
          {% endif %}
```

In this example, `{{ monitor_name }}`, `{{ block_number }}`, `{{ transaction_hash }}`, `{{ log.address }}`, `{{ log.log_index }}`, `{{ log.name }}`, and `{{ log.params }}` are placeholders that will be replaced with the actual data at the time of notification. The `{% if decoded_call %}` block conditionally includes function call information when available.

## Example: Slack Notification with Aggregation Policy

When using an `aggregation` policy, the template has access to a `matches` array, which contains all the `MonitorMatch` objects collected during the aggregation window.

```yaml
actions:
  - name: "slack-with-aggregation"
    slack:
      slack_url: "https://hooks.slack.com/services/T00000000/B00000000/XXXXXXXXXXXXXXXXXXXXXXXX"
      message:
        title: "Aggregated Event Summary"
        body: "This is a summary of events."
    policy:
      aggregation:
        window_secs: 300 # 5 minutes
        template:
          title: "Event Summary for {{ monitor_name }}"
          body: |
            In the last 5 minutes there have been {{ matches | length }} new events
            {% for match in matches %}
            - Tx: {{ match.transaction_hash }} (Block: {{ match.block_number }})
            {% if match.decoded_call %}
              Function: {{ match.decoded_call.name }}({{ match.decoded_call.params | tojson }})
            {% endif %}
            {% endfor %}
```

Here, `{{ matches | length }}` is used to get the count of aggregated matches, and a `{% for match in matches %}` loop iterates through each individual match to display its transaction hash and block number. The example also shows how to access `decoded_call` data for each match in the aggregation.

Refer to the documentation for specific action types to understand their full templating capabilities and the exact syntax to use.

For more practical examples of action templating, including aggregation policies, refer to the [Example Gallery](../examples/gallery.md).
