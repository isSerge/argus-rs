# 12. Action with NATS Publisher

This example demonstrates how to configure and use a `nats` action to send
notifications to a NATS subject. It includes a `docker-compose.yml` to easily
spin up a local NATS server for testing.

### Configuration Files

- [`docker-compose.yml`](./docker-compose.yml): A minimal setup to run a single-node NATS server locally using the lightweight `nats:alpine` image.
- [`app.yaml`](../../docs/src/user_guide/config_app.md): Basic application configuration.
- [`monitors.yaml`](../../docs/src/user_guide/config_monitors.md): Defines the "Large ETH Transfers to NATS" monitor.
- [`actions.yaml`](../../docs/src/user_guide/config_actions.md): Defines a `nats` action that sends notifications to a local NATS server.

### Prerequisites

- [Docker](https://www.docker.com/get-started) and [Docker Compose](https://docs.docker.com/compose/install/) installed.
- [NATS CLI](https://github.com/nats-io/natscli) (optional, for verifying messages).

### How to Run

#### 1. Start NATS Server

First, start the NATS server using the provided `docker-compose.yml`:

```bash
docker compose -f examples/12_action_with_nats_publisher/docker-compose.yml up -d
```

This will start a NATS server in detached mode, with the client protocol listening on `localhost:4222` and the monitoring UI on `http://localhost:8222`.

#### 2. Run the Monitor (Dry-Run Mode)

To test this monitor against historical blocks, use the `dry-run` command:

```bash
cargo run --release -- dry-run --from 23159290 --to 23159291 --config-dir examples/12_action_with_nats_publisher/
```

Run with `debug` logs:

```bash
RUST_LOG=debug cargo run --release -- dry-run --from 23159290 --to 23159291 --config-dir examples/12_action_with_nats_publisher/
```

#### 3. Verify the Output in NATS

While the `dry-run` is in progress or after it has finished, you can check the messages published to the NATS subject.

You can use the NATS CLI to subscribe to the subject and see the messages:

1.  Install the NATS CLI if you haven't already.
2.  Subscribe to the `large.eth.transfers` subject:

    ```bash
    nats sub large.eth.transfers
    ```

3.  Now, re-run the `dry-run` command. You should see the message payloads printed to your terminal by the `nats sub` command:

```
15:03:11 Subscribing on large.eth.transfers
[#1] Received on "large.eth.transfers"
X-Message-Key: 0x92a19bc7912f993ed94faa3ea102f4fd244aaf78f5439071bd1126ab419f2ce6

{"action_name":"nats-action","block_number":23159291,"monitor_id":1,"monitor_name":"Large ETH Transfers to NATS","transaction_hash":"0x92a19bc7912f993ed94faa3ea102f4fd244aaf78f5439071bd1126ab419f2ce6","tx":{"from":"0x30F2864e7bf6E89a3955217C78c8689594228940","gas_limit":21000,"gas_price":"2000000000","hash":"0x92a19bc7912f993ed94faa3ea102f4fd244aaf78f5439071bd1126ab419f2ce6","input":"0x","nonce":5361,"to":"0xCFFAd3200574698b78f32232aa9D63eABD290703","transaction_index":46,"value":"13859958000000000000"}}


[#2] Received on "large.eth.transfers"
X-Message-Key: 0x36a00cbdec61ed072e4a5e6ab0af1cbbb72f13cf8dbdc6b313c52ebc9abc6a19

{"action_name":"nats-action","block_number":23159291,"monitor_id":1,"monitor_name":"Large ETH Transfers to NATS","transaction_hash":"0x36a00cbdec61ed072e4a5e6ab0af1cbbb72f13cf8dbdc6b313c52ebc9abc6a19","tx":{"from":"0x133eEf17Bf97C3ca9647E206D9c3Ac41Fd345a20","gas_limit":25500,"gas_price":"1000000000","hash":"0x36a00cbdec61ed072e4a5e6ab0af1cbbb72f13cf8dbdc6b313c52ebc9abc6a19","input":"0x","nonce":50,"to":"0xA33D00CF429CE29691C8215259B4Bb749EF70e29","transaction_index":54,"value":"25000000000000000000"}}
```

You can also check the server stats via the monitoring endpoint at `http://localhost:8222/`.

#### 4. Stop and Clean Up

Once you are finished, you can stop and remove the NATS container:

```bash
docker compose -f examples/12_action_with_nats_publisher/docker-compose.yml down
```
