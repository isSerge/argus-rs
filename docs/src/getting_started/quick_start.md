# Quick Start

This guide will walk you through the essential steps to configure and run your first Argus monitor using Docker Compose.

## Prerequisites

Ensure you have completed the [Docker installation steps](./installation.md), including cloning the repository, creating your `.env` file, and creating the `data` directory.

## 1. Review Application Configuration (`app.yaml`)

The `configs/app.yaml` file contains the core settings for the application. The most critical settings are the RPC endpoints. The default file includes public endpoints for Ethereum mainnet, which are fine for this quick start.

```yaml
# configs/app.yaml
database_url: "sqlite:argus.db"
rpc_urls:
  - "https://eth.llamarpc.com"
  - "https://1rpc.io/eth"
network_id: "ethereum"
# ... other settings
```
**Note**: The `database_url` is relative to the container's working directory. The `docker compose.yml` file mounts the local `./data` directory to `/app`, so the database file will be created at `./data/argus.db` on your host machine.

## 2. Define a Monitor (`monitors.yaml`)

The repository provides example configurations. Let's copy them to create your local, editable versions.

```bash
cp configs/monitors.example.yaml configs/monitors.yaml
cp configs/notifiers.example.yaml configs/notifiers.yaml
```

Now, open `configs/monitors.yaml`. For this example, we'll use the pre-configured "Large ETH Transfers" monitor.

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

This monitor will trigger for any transaction on `ethereum` where more than 10 ETH is transferred. It will send a notification using the `my-webhook` notifier.

## 3. Configure a Notifier (`notifiers.yaml`)

Finally, let's configure *how* we get notified. Open `configs/notifiers.yaml`.

To receive alerts, you'll need a webhook endpoint. For testing, you can use a service like [Webhook.site](https://webhook.site/) to get a temporary URL.

Update the `url` in the `my-webhook` notifier configuration with your actual webhook URL. **Remember to use environment variables for secrets!**

```yaml
# configs/notifiers.yaml
notifiers:
  - name: "my-webhook"
    webhook:
      url: "${WEBHOOK_URL}" # <-- SET THIS IN YOUR .env FILE
      message:
        title: "Large ETH Transfer Detected"
        body: |
          - **Amount**: {{ tx.value | ether }} ETH
          - **From**: `{{ tx.from }}`
          - **To**: `{{ tx.to }}`
          - **Tx Hash**: `{{ transaction_hash }}`
```
Now, open your `.env` file and add the `WEBHOOK_URL`:
```env
# .env
WEBHOOK_URL=https://webhook.site/your-unique-url
```

## 4. Run Argus

With the configuration in place, you are now ready to start the monitoring service.

Run the following command from the root of the project:

```bash
docker compose up -d
```

Argus will start, automatically run database migrations, connect to the RPC endpoint, and begin processing new blocks. When a transaction matches your filter (a transfer of >10 ETH), a notification will be sent to the webhook URL you configured.

You can view the application's logs with:
```bash
docker compose logs -f
```

To stop the service:
```bash
docker compose down
```
