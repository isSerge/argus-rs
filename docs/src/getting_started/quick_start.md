# Quick Start

This guide will walk you through the essential steps to configure and run your first Argus monitor. We'll create a simple monitor that watches for large ETH transfers on the Ethereum mainnet.

## 1. Create Your Configuration

Argus is configured using [`app.yaml`](../user_guide/app_yaml.md), [`monitors.yaml`](../user_guide/monitors_yaml.md), and [`notifiers.yaml`](../user_guide/notifiers_yaml.md) located in a `configs` directory. The repository provides `monitors.example.yaml` and `notifiers.example.yaml` to serve as templates.

Let's create your local configuration files by copying the examples:

```bash
cp configs/monitors.example.yaml configs/monitors.yaml
cp configs/notifiers.example.yaml configs/notifiers.yaml
```

Now you will have your own `monitors.yaml` and `notifiers.yaml` that you can edit safely.

## 2. Application Configuration ([`app.yaml`](../user_guide/app_yaml.md))

The `app.yaml` file contains the core settings for the application. The most critical settings are the RPC endpoints.

Open `configs/app.yaml` and ensure the `rpc_urls` list contains at least one valid Ethereum RPC endpoint. The default examples are good starting points.

```yaml
# configs/app.yaml
database_url: "sqlite:argus.db"
rpc_urls:
  - "https://eth.llamarpc.com"
  - "https://1rpc.io/eth"
network_id: "ethereum"
# ... other settings
```

## 3. Database Setup

Argus needs a local SQLite database to store its state. The `database_url` in [`app.yaml`](../user_guide/app_yaml.md) points to the file that will be used.

Before running the application for the first time, you must run the database migrations. This will create the necessary tables.

**Prerequisite**: Ensure you have `sqlx-cli` installed. If not, you can install it with `cargo install sqlx-cli`.

```bash
sqlx migrate run
```

You should see output indicating that the migrations have been successfully applied.

## 4. Define a Monitor ([`monitors.yaml`](../user_guide/monitors_yaml.md))

Now, let's define what we want to monitor. Open `configs/monitors.yaml`. For this example, we'll use the pre-configured "Large ETH Transfers" monitor.

```yaml
# configs/monitors.yaml
monitors:
  - name: "Large ETH Transfers"
    network: "ethereum"
    filter_script: |
      tx.value > ether(10)
    notifiers:
      - "my-webhook"
```

This monitor will trigger for any transaction on the `ethereum` where more than 10 ETH is transferred. It will send a notification using the `my-webhook` notifier.

## 5. Configure a Notifier ([`notifiers.yaml`](../user_guide/notifiers_yaml.md))

Finally, let's configure *how* we get notified. Open `configs/notifiers.yaml`.

To receive alerts, you'll need a webhook endpoint. For testing, you can use a service like [Webhook.site](https://webhook.site/) to get a temporary URL.

Update the `url` in the `my-webhook` notifier configuration with your actual webhook URL.

```yaml
# configs/notifiers.yaml
notifiers:
  - name: "my-webhook"
    webhook:
      url: "https://webhook.site/your-unique-url" # <-- CHANGE THIS
      message:
        title: "Large ETH Transfer Detected"
        body: |
          - **Amount**: {{ tx.value }} ETH
          - **From**: `{{ tx.from }}`
          - **To**: `{{ tx.to }}`
          - **Tx Hash**: `{{ transaction_hash }}`
```

## 6. Run Argus

With the configuration in place, you are now ready to start the monitoring service.

Run the following command from the root of the project:

```bash
cargo run --release -- run
```

Argus will start, connect to the RPC endpoint, and begin processing new blocks. When a transaction matches your filter (a transfer of >10 ETH), a notification will be sent to the webhook URL you configured.
