# 3. WETH Deposit Monitor

This example sets up a monitor that triggers when a `Deposit` event is detected
from the WETH contract on the Ethereum mainnet. It uses a Telegram notifier.

### Configuration Files

- [`app.yaml`](../../docs/src/user_guide/config_app.md): Basic application configuration, pointing to public RPC endpoints.
- [`monitors.yaml`](../../docs/src/user_guide/config_monitors.md): Defines the "WETH Deposits" monitor.
- [`notifiers.yaml`](../../docs/src/user_guide/config_notifiers.md): Defines "Telegram WETH Deposits" notifier.

### Environment Variables for Notifier Secrets

> **Important:** All secrets and sensitive values in `notifiers.yaml` (such as API tokens, webhook URLs, chat IDs, etc.) must be provided as environment variables.
> For example, if your `notifiers.yaml` contains:
>
> ```yaml
> token: "${TELEGRAM_TOKEN}"
> chat_id: "${TELEGRAM_CHAT_ID}"
> ```
>
> You must set these in your shell before running Argus:
>
> ```sh
> export TELEGRAM_TOKEN="your-telegram-token"
> export TELEGRAM_CHAT_ID="your-chat-id"
> ```
>
> See the example `notifiers.yaml` for all required variables for each notifier type.

### Monitor Configuration

The `monitors.yaml` file in this example defines a single monitor. For a complete reference on monitor configuration, see the [Monitor Configuration documentation](../../docs/src/user_guide/config_monitors.md).

```yaml
monitors:
  - name: 'WETH Deposits'
    network: 'ethereum'
    address: '0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2'
    abi: 'weth'
    filter_script: |
      log.name == "Deposit" && tx.value > ether(5)
    notifiers:
      - 'Telegram WETH Deposits'
```

- **`name`**: A human-readable name for the monitor.
- **`network`**: Specifies the blockchain network to monitor (e.g., "ethereum").
  This must match a network configured in `app.yaml`.
- **`address`**: The contract address of the WETH token on Ethereum mainnet.
  This ensures the monitor only processes events from this specific contract. For more details on `address` configuration, see the [Monitor Configuration documentation](../../docs/src/user_guide/config_monitors.md#monitor-fields).
- **`abi`**: The name of the ABI (Application Binary Interface) to use for
  decoding contract events. Here, "weth" refers to the `weth.json` file in the
  `abis/` directory. This is crucial for `log.name` and `log.params` to be
  available in the `filter_script`. For more details, see [ABI Management](../../docs/src/user_guide/config_abis.md).
- **`filter_script`**: This [Rhai script](../../docs/src/user_guide/rhai_scripts.md) defines the conditions for a match.
  `log.name == "Deposit"` checks for the `Deposit` event, `tx.value > ether(5)`
  checks if the transaction's value is greater than 5 ETH. The [`ether()`](../../docs/src/user_guide/rhai_helpers.md#ethervalue) function is a
  convenient wrapper, which handles `BigInt` conversions. For more on `log` and `tx` data, see [Rhai Data Context](../../docs/src/user_guide/rhai_context.md).
- **`notifiers`**: A list of notifier names (defined in `notifiers.yaml`) that
  will receive alerts when this monitor triggers. Here, it references "Telegram
  WETH Deposits".

### Notifier Configuration

The `notifiers.yaml` in this example defines a single Telegram notifier. For a complete reference on notifier configuration, see the [Notifier Configuration documentation](../../docs/src/user_guide/config_notifiers.md).

```yaml
notifiers:
  - name: 'Telegram WETH Deposits'
    telegram:
      token: '<TELEGRAM TOKEN>'
      chat_id: '<TELEGRAM CHAT ID>'
      disable_web_preview: true
      message:
        title: 'WETH Deposit Detected'
        body: |
          A WETH deposit was detected by monitor {{ monitor_name }}.
          - *From*: `{{ log.params.dst }}`                     
          - *Value*: `{{ log.params.wad | ether }}` WETH 
          [View on Etherscan](https://etherscan.io/tx/{{ transaction_hash }})
```

- **`name`**: A unique, human-readable name for the notifier. This name is
  referenced by monitors in their `notifiers` list.
- **`telegram`**: This block configures a Telegram notifier.
  - **`token`**: Your Telegram bot token.
  - **`chat_id`**: The ID of the Telegram chat where notifications will be sent.
  - **`disable_web_preview`**: (Optional) Set to `true` to disable link previews
    in Telegram messages.
  - **`message`**: Defines the structure and content of the notification
    message. For more details on templating, see the [Notifier Templating documentation](../../docs/src/user_guide/notifier_templating.md).
    - **`title`**: The title of the notification. Supports
      [Jinja2-like templating](https://docs.rs/minijinja/latest/minijinja/) to
      include dynamic data from the monitor match (e.g., `{{ monitor_name }}`).
    - **`body`**: The main content of the notification. In this example we use
      event log fields `dst` (the address that initiated the deposit) and `wad`
      (the amount of WETH deposited). For more on `log` data, see [Rhai Data Context](../../docs/src/user_guide/rhai_context.md#the-log-object-decoded-event-log).

### How to Run ([Dry-Run Mode](../../docs/src/operations/cli.md#dry-run-mode))

To test this monitor against historical blocks, use the `dry-run` command with
the `--config-dir` argument pointing to this example's configuration:

```bash
cargo run --release -- dry-run --from 18000003 --to 18000004 --config-dir examples/3_weth_deposit/
```

Run with `debug` logs:

```bash
RUST_LOG=debug cargo run --release -- dry-run --from 18000003 --to 18000004 --config-dir examples/3_weth_deposit/
```

Run with Docker image from GHCR:

```bash
docker run --rm \
  --env-file .env \
  -v "$(pwd)/examples/3_weth_deposit:/app/configs:ro" \
  -v "$(pwd)/abis:/app/abis:ro" \
  ghcr.io/isserge/argus-rs:latest \
  dry-run --from 18000003 --to 18000004 --config-dir /app/configs
```

Replace `18000003` and `18000004` with any Ethereum block numbers to test
against.

#### Expected Output

As blocks within the specified range are processed, you should receive
notifications on Telegram (or another specified notifier):

![Sample notification output (Telegram)](image.png)

Once processing is complete, you should see the following output in your
terminal, which is a JSON array with all detected monitor matches:

```json
[
  {
    "monitor_id": 0,
    "monitor_name": "WETH Deposits",
    "notifier_name": "Telegram WETH Deposits",
    "block_number": 18000004,
    "transaction_hash": "0xf180607da1c35e13bdb91ac8006337774812c34075ed1cdd9a7765f5197b4882",
    "tx": {
      "from": "0x8EFef6B061bdA1a1712DB316E059CBc8eBDCAE4D",
      "gas_limit": 374650,
      "hash": "0xf180607da1c35e13bdb91ac8006337774812c34075ed1cdd9a7765f5197b4882",
      "input": "0x3593564c000000000000000000000000000000000000000000000000000000000000006000000000000000000000000000000000000000000000000000000000000000a00000000000000000000000000000000000000000000000000000000064ea2daf00000000000000000000000000000000000000000000000000000000000000030b000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000003000000000000000000000000000000000000000000000000000000000000006000000000000000000000000000000000000000000000000000000000000000c000000000000000000000000000000000000000000000000000000000000001e0000000000000000000000000000000000000000000000000000000000000004000000000000000000000000000000000000000000000000000000000000000020000000000000000000000000000000000000000000000006124fee993bc0000000000000000000000000000000000000000000000000000000000000000010000000000000000000000000000000000000000000000000000000000000000010000000000000000000000000000000000000000000000005292a579bd9300000000000000000000000000000000000000000000000000990a8792e62020b71300000000000000000000000000000000000000000000000000000000000000a00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000002bc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2000bb8b23d80f5fefcddaa212212f028021b41ded428cf000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000012000000000000000000000000000000000000000000000000000000000000000010000000000000000000000000000000000000000000000000e92596fd629000000000000000000000000000000000000000000000000001b38698dd86e4df7dd00000000000000000000000000000000000000000000000000000000000000a000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000042c02aaa39b223fe8d0a0e5c4f27ead9083c756cc2002710375abb85c329753b1ba849a601438ae77eec9893002710b23d80f5fefcddaa212212f028021b41ded428cf000000000000000000000000000000000000000000000000000000000000",
      "max_fee_per_gas": "29728245039",
      "max_priority_fee_per_gas": "100000000",
      "nonce": 4492,
      "to": "0x3fC91A3afd70395Cd496C647d5a6CC9D4B2b7FAD",
      "transaction_index": 1,
      "value": "7000000000000000000"
    },
    "log": {
      "address": "0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2",
      "log_index": 3,
      "name": "Deposit",
      "params": {
        "dst": "0x3fC91A3afd70395Cd496C647d5a6CC9D4B2b7FAD",
        "wad": 7000000000000000000
      }
    }
  },
  {
    "monitor_id": 0,
    "monitor_name": "WETH Deposits",
    "notifier_name": "Telegram WETH Deposits",
    "block_number": 18000004,
    "transaction_hash": "0xf54bf6bfa5ec33fc7ba58fdcde27bc5987b48a8ca6b0ca49c8e9d7e42738e42d",
    "tx": {
      "from": "0x96223E34f5071De807757b7A7fa129D4d5aDe20C",
      "gas_limit": 207329,
      "hash": "0xf54bf6bfa5ec33fc7ba58fdcde27bc5987b48a8ca6b0ca49c8e9d7e42738e42d",
      "input": "0xe449022e0000000000000000000000000000000000000000000000055de6a779bbac0000000000000000000000000000000000000000000000000004f22a4f6b5bb67b4700000000000000000000000000000000000000000000000000000000000000600000000000000000000000000000000000000000000000000000000000000001c00000000000000000000000a4e0faa58465a2d369aa21b3e42d43374c6f9613e26b9977",
      "max_fee_per_gas": "21118047667",
      "max_priority_fee_per_gas": "250000000",
      "nonce": 810,
      "to": "0x1111111254EEB25477B68fb85Ed929f73A960582",
      "transaction_index": 77,
      "value": "99000000000000000000"
    },
    "log": {
      "address": "0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2",
      "log_index": 217,
      "name": "Deposit",
      "params": {
        "dst": "0x1111111254EEB25477B68fb85Ed929f73A960582",
        "wad": "99000000000000000000"
      }
    }
  }
]
```

### How to Run (Default Mode)

Once you have verified your monitor works against historical data in `dry-run`
mode, you can start it in default (live monitoring) mode. In this mode, the
monitor will continuously poll for new blocks and dispatch actual notifications
via the configured notifier when a match is found.

```bash
cargo run --release -- run --config-dir examples/3_weth_deposit/
```

Using Docker image from GHCR:

```bash
# First, create a data directory for this example
mkdir -p examples/3_weth_deposit/data

# Run the container in detached mode
docker run --rm -d \
  --name argus_example_3 \
  --env-file .env \
  -v "$(pwd)/examples/3_weth_deposit:/app/configs:ro" \
  -v "$(pwd)/abis:/app/abis:ro" \
  -v "$(pwd)/examples/3_weth_deposit/data:/app/data" \
  ghcr.io/isserge/argus-rs:latest \
  run --config-dir /app/configs
```
