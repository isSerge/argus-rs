# 10. Action with Kafka Publisher

This example demonstrates how to configure and use a `kafka` action to send
notifications to a Kafka topic. It includes a `docker-compose.yml` to easily
spin up a local Kafka broker for testing.

### Configuration Files

- [`docker-compose.yml`](./docker-compose.yml): A minimal setup to run a single-node Kafka broker locally using the lightweight, multi-arch `itzg/kafka` image.
- [`app.yaml`](../../docs/src/user_guide/config_app.md): Basic application configuration.
- [`monitors.yaml`](../../docs/src/user_guide/config_monitors.md): Defines the "Large ETH Transfers to Kafka" monitor.
- [`actions.yaml`](../../docs/src/user_guide/config_actions.md): Defines a `kafka` action that sends notifications to a local Kafka broker.

### Prerequisites

- [Docker](https://www.docker.com/get-started) and [Docker Compose](https://docs.docker.com/compose/install/) installed.
- A Kafka client CLI for consuming messages, such as [`kcat`](https://github.com/edenhill/kcat) (previously kafkacat). You can install it with Homebrew (`brew install kcat`) or other package managers.

### How to Run

#### 1. Start Kafka Broker

First, start the Kafka broker using the provided `docker-compose.yml`:

```bash
docker compose -f examples/10_action_with_kafka_publisher/docker-compose.yml up -d
```

This will start a Kafka broker in detached mode, listening on `localhost:9092`.

#### 2. Run the Monitor (Dry-Run Mode)

To test this monitor against historical blocks, use the `dry-run` command:

```bash
cargo run --release -- dry-run --from 23159290 --to 23159291 --config-dir examples/10_action_with_kafka_publisher/
```

Run with `debug` logs:

```bash
RUST_LOG=debug cargo run --release -- dry-run --from 23159290 --to 23159291 --config-dir examples/10_action_with_kafka_publisher/
```

#### 3. Verify the Output in Kafka

While the `dry-run` is in progress or after it has finished, you can consume the messages from the `argus-alerts` topic in a separate terminal to verify that the notifications were sent:

```bash
kcat -b localhost:9092 -t argus-alerts -C -J
```

- `-b localhost:9092`: Specifies the broker address.
- `-t argus-alerts`: Specifies the topic to consume from.
- `-C`: Puts `kcat` in consumer mode.
- `-J`: Outputs the message payload as pretty-printed JSON.

##### Expected Kafka Output

You should see JSON payloads in your terminal for each notification sent by the monitor. Each payload will contain the context of the monitor match, like this:

```json
[
  {
    "topic": "argus-alerts",
    "partition": 0,
    "offset": 0,
    "tstype": "create",
    "ts": 1759203209616,
    "broker": 0,
    "key": "0x41b3b3507a407ddc44937c6de585af5bf4c5c34619aff1db0306ccd9e10c212c",
    "payload": "{\"action_name\":\"kafka-action\",\"block_number\":23289381,\"monitor_id\":1,\"monitor_name\":\"Large ETH Transfers to Kafka\",\"transaction_hash\":\"0x41b3b3507a407ddc44937c6de585af5bf4c5c34619aff1db0306ccd9e10c212c\",\"tx\":{\"from\":\"0xeF317e433B0836F294866d43f67D6871b609B351\",\"gas_limit\":90000,\"hash\":\"0x41b3b3507a407ddc44937c6de585af5bf4c5c34619aff1db0306ccd9e10c212c\",\"input\":\"0x\",\"max_fee_per_gas\":\"2575354180\",\"max_priority_fee_per_gas\":\"1400000000\",\"nonce\":31744,\"to\":\"0x475446124a015be6cd139FeA1B2A9Caa782Ef305\",\"transaction_index\":108,\"value\":\"10644517600000000000\"}}"
  },
  {
    "topic": "argus-alerts",
    "partition": 0,
    "offset": 1,
    "tstype": "create",
    "ts": 1759203210481,
    "broker": 0,
    "key": "0x8f919021847400e294fa88b9829a74c6ccec903916f5659d60749671e6662c0d",
    "payload": "{\"action_name\":\"kafka-action\",\"block_number\":23289383,\"monitor_id\":1,\"monitor_name\":\"Large ETH Transfers to Kafka\",\"transaction_hash\":\"0x8f919021847400e294fa88b9829a74c6ccec903916f5659d60749671e6662c0d\",\"tx\":{\"from\":\"0x1AB4973a48dc892Cd9971ECE8e01DcC7688f8F23\",\"gas_limit\":200000,\"hash\":\"0x8f919021847400e294fa88b9829a74c6ccec903916f5659d60749671e6662c0d\",\"input\":\"0x\",\"max_fee_per_gas\":\"15996903880\",\"max_priority_fee_per_gas\":\"100076020\",\"nonce\":1954545,\"to\":\"0x933B5912611c9Ba107A8cF7853B244492fF6a184\",\"transaction_index\":269,\"value\":\"10069704220000000000\"}}"
  }
]
```

#### 4. Stop and Clean Up

Once you are finished, you can stop and remove the Kafka container:

```bash
docker compose -f examples/10_action_with_kafka_publisher/docker-compose.yml down
```
