# `actions.yaml` Configuration

The `actions.yaml` file defines *how* and *where* you receive alerts when a monitor's conditions are met. You can configure multiple actions for different services and purposes.

Each action has a unique `name`, a `type` (e.g., `webhook`, `slack`), and a set of configuration options specific to that type.

> ### **IMPORTANT: Managing Secrets**
>
> Action configurations, especially for services like Slack, Discord, or Telegram, often require secret URLs or API tokens.
>
> **Never commit secrets to version control.**
>
> This project is set up to help you. The `configs/actions.yaml` file is already listed in the project's `.gitignore` file. This is a deliberate security measure to prevent you from accidentally committing sensitive information. When you copy `actions.example.yaml` to `actions.yaml`, your file with secrets will be ignored by Git automatically.

## Common Action Structure

```yaml
actions:
  - name: "unique-action-name"
    # Action type and its specific configuration
    webhook:
      url: "..."
      # ...
    # Optional policy to control notification frequency
    policy:
      # ...
```

---

## Action Types

### Webhook

The `webhook` action sends a generic HTTP POST request to a specified URL. This is the most flexible action and can be used to integrate with a wide variety of services.

```yaml
- name: "my-generic-webhook"
  webhook:
    # The URL of your webhook endpoint.
    url: "https://my-service.com/webhook-endpoint"
    # (Optional) The HTTP method to use. Defaults to "POST".
    method: "POST"
    # (Optional) A secret key to sign the request payload with HMAC-SHA256.
    # Included in the `X-Signature` header.
    secret: "your-super-secret-webhook-secret"
    # (Optional) Custom headers to include in the request.
    headers:
      Authorization: "Bearer your-auth-token"
    # The message to send. Both `title` and `body` support templating.
    message:
      title: "New Alert: {{ monitor_name }}"
      body: "A match was detected on block {{ block_number }} for tx: {{ transaction_hash }}"
```

### Slack

Sends a message to a Slack channel via an Incoming Webhook.

```yaml
- name: "slack-notifications"
  slack:
    # Your Slack Incoming Webhook URL.
    slack_url: "https://hooks.slack.com/services/T0000/B0000/XXXXXXXX"
    message:
      title: "Large USDC Transfer Detected"
      body: |
        A transfer of over 1,000,000 USDC was detected.
        <https://etherscan.io/tx/{{ transaction_hash }}|View on Etherscan>
```

### Discord

Sends a message to a Discord channel via a webhook.

```yaml
- name: "discord-alerts"
  discord:
    # Your Discord Webhook URL.
    discord_url: "https://discord.com/api/webhooks/0000/XXXXXXXX"
    message:
      title: "WETH Deposit Event"
      body: "A new WETH deposit was detected for tx `{{ transaction_hash }}`."
```

### Telegram

Sends a message to a Telegram chat via a bot.

```yaml
- name: "telegram-updates"
  telegram:
    # Your Telegram Bot Token.
    token: "0000:XXXXXXXX"
    # The ID of the chat to send the message to.
    chat_id: "-1000000000"
    message:
      title: "Large Native Token Transfer"
      body: |
        A transfer of over 10 ETH was detected.
        [View on Etherscan](https://etherscan.io/tx/{{ transaction_hash }})
```

### Stdout

Prints the notification to standard output (the console). This is primarily useful for local development, testing, and debugging.

If a `message` template is provided, it will be rendered and printed. If `message` is omitted, the full, raw `MonitorMatch` JSON payload will be printed.

```yaml
- name: "stdout-for-debugging"
  stdout:
    # Message is optional for stdout. If omitted, the full event JSON payload is printed.
    message:
      title: "Debug Event: {{ monitor_name }}"
      body: "tx hash: {{ transaction_hash }}"
```

### Kafka

The `kafka` action sends the full `MonitorMatch` JSON payload to a specified Apache Kafka topic.

```yaml
- name: "kafka-action"
  kafka:
    # A comma-separated list of Kafka broker addresses.
    brokers: "127.0.0.1:9092"
    # The Kafka topic to publish messages to.
    topic: "argus-alerts"
    # (Optional) Security configuration for connecting to Kafka.
    security:
      protocol: "SASL_SSL"
      sasl_mechanism: "PLAIN"
      sasl_username: "${KAFKA_USERNAME}"
      sasl_password: "${KAFKA_PASSWORD}"
      ssl_ca_location: "/path/to/ca.crt"
    # (Optional) Producer-specific configuration properties.
    producer:
      message_timeout_ms: 5000
      compression_codec: "snappy"
      acks: "all"
```

**Configuration Details:**

*   `brokers` (string, required): A comma-separated list of Kafka broker addresses (e.g., `"broker1:9092,broker2:9092"`).
*   `topic` (string, required): The Kafka topic to publish messages to.
*   `security` (object, optional): Configuration for connecting to a secure Kafka cluster.
    *   `protocol` (string): The security protocol to use. Common values are `PLAINTEXT`, `SSL`, `SASL_PLAINTEXT`, `SASL_SSL`. Defaults to `PLAINTEXT`.
    *   `sasl_mechanism` (string, optional): The SASL mechanism for authentication (e.g., `PLAIN`, `SCRAM-SHA-256`).
    *   `sasl_username` (string, optional): The username for SASL authentication. Supports environment variable expansion (e.g., `${KAFKA_USERNAME}`).
    *   `sasl_password` (string, optional): The password for SASL authentication. Supports environment variable expansion.
    *   `ssl_ca_location` (string, optional): Path to the CA certificate file for verifying the broker's certificate.
*   `producer` (object, optional): Advanced configuration for the Kafka producer.
    *   `message_timeout_ms` (integer): The maximum time in milliseconds to wait for a message to be sent. Defaults to `5000`.
    *   `compression_codec` (string): The compression codec to use. Common values are `none`, `gzip`, `snappy`, `lz4`, `zstd`. Defaults to `none`.
    *   `acks` (string): The number of acknowledgments required before a request is considered complete. Can be `0`, `1`, or `all`. Defaults to `all` for maximum durability.

### RabbitMQ

The `rabbitmq` action sends the full `MonitorMatch` JSON payload to a RabbitMQ exchange.

```yaml
- name: "rabbitmq-action"
  rabbitmq:
    # The RabbitMQ connection URI.
    uri: "amqp://guest:guest@127.0.0.1:5672/%2f"
    # The name of the exchange to publish messages to.
    exchange: "argus-alerts-exchange"
    # The type of the exchange (e.g., "topic", "direct", "fanout").
    exchange_type: "topic"
    # The routing key to use for the message.
    routing_key: "large.eth.transfers"
```

### NATS

The `nats` action sends the full `MonitorMatch` JSON payload to a NATS subject.

```yaml
- name: "nats-action"
  nats:
    # The NATS connection URL(s), comma-separated.
    urls: "nats://127.0.0.1:4222"
    # The subject to publish messages to.
    subject: "argus.alerts"
    # (Optional) Credentials for connecting to NATS.
    credentials:
      # (Optional) A token for authentication.
      token: "${NATS_TOKEN}"
      # (Optional) Path to a credentials file (.creds).
      file: "/path/to/user.creds"
```

---

## Notification Policies

Policies allow you to control the rate and structure of your notifications, helping to reduce noise and provide more meaningful alerts.

### Throttle Policy

The `throttle` policy limits the number of notifications sent within a specified time window. This is useful for high-frequency events.

```yaml
- name: "discord-with-throttling"
  discord:
    # ... discord config
  policy:
    throttle:
      # Max notifications to send within the time window.
      max_count: 5
      # The duration of the time window in seconds.
      time_window_secs: 60 # 5 notifications per minute
```

### Aggregation Policy

The `aggregation` policy collects all matches that occur within a time window and sends a single, consolidated notification. This is ideal for summarizing events. When using an aggregation policy, you can leverage custom filters like `map`, `sum`, and `avg` on the `matches` array to perform calculations.

```yaml
- name: "slack-with-aggregation"
  slack:
    # ... slack config
  policy:
    aggregation:
      # The duration of the aggregation window in seconds.
      window_secs: 300 # 5 minutes
      # The template for the aggregated notification.
      # This template has access to a `matches` array, which contains all the
      # `MonitorMatch` objects collected during the window.
      template:
        title: "Event Summary for {{ monitor_name }}"
        body: |
          Detected {{ matches | length }} events by monitor {{ monitor_name }}.
          Total value: {{ matches | map(attribute='log.params.value') | sum | wbtc }} WBTC
          Average value: {{ matches | map(attribute='log.params.value') | avg | wbtc }} WBTC
```

In this example:
*   `matches | length` counts the number of aggregated events.
*   `matches | map(attribute='log.params.value')` extracts the `log.params.value` (which is a `BigInt` string) from each match in the `matches` array.
*   `sum` calculates the total of the extracted values.
*   `avg` calculates the average of the extracted values.
*   `wbtc` is a critical custom filter that converts the `BigInt` string value into a decimal representation (e.g., WBTC units) before mathematical operations are performed. Without this conversion, `sum` and `avg` would operate on the raw `BigInt` strings, leading to incorrect results.

---

## Templating

Action messages support [Jinja2](https://jinja.palletsprojects.com/) templating, allowing for dynamic content based on the detected blockchain events. For a comprehensive guide on available data, conversion filters, and examples, refer to the [Action Templating documentation](./action_templating.md).
