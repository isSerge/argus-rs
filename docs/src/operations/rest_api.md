# REST API

Argus includes a built-in REST API server for system introspection. This API provides a way to observe the state of the running application, such as its health and the configuration of its active monitors.

## Enabling the API

For security, the API server is **disabled by default**. To enable it, you must configure the `server` section in your [`app.yaml`](../user_guide/app_yaml.md).

```yaml
# in configs/app.yaml
server:
  # Set to true to enable the API server.
  enabled: true
  # (Optional) The address and port for the server to listen on.
  listen_address: "0.0.0.0:8080"
```

Once enabled, the API endpoints will be available at the specified `listen_address`.

## API Endpoints

### Health Check

-   **`GET /health`**

    Provides a simple health check of the API server.

    **Success Response (`200 OK`)**
    ```json
    {
      "status": "ok"
    }
    ```

    **Example Usage:**
    ```bash
    curl http://localhost:8080/health
    ```

### List All Monitors

-   **`GET /monitors`**

    Retrieves a list of all monitors currently loaded and active in the application for the configured network.

    **Success Response (`200 OK`)**
    ```json
    {
      "monitors": [
        {
          "id": 1,
          "name": "Large ETH Transfers",
          "network": "ethereum",
          "address": null,
          "abi": null,
          "filter_script": "tx.value > ether(10)",
          "actions": [
            "my-webhook"
          ],
          "created_at": "2023-10-27T10:00:00Z",
          "updated_at": "2023-10-27T10:00:00Z"
        },
        {
          "id": 2,
          "name": "Large USDC Transfers",
          "network": "ethereum",
          "address": "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48",
          "abi": "usdc",
          "filter_script": "log.name == \"Transfer\" && log.params.value > usdc(1000000)",
          "actions": [
            "slack-notifications"
          ],
          "created_at": "2023-10-27T10:00:00Z",
          "updated_at": "2023-10-27T10:00:00Z"
        }
      ]
    }
    ```

    **Example Usage:**
    ```bash
    curl http://localhost:8080/monitors
    ```

### Get a Specific Monitor

-   **`GET /monitors/{id}`**

    Retrieves the full configuration of a single monitor by its unique ID.

    **URL Parameters:**
    - `id` (integer, required): The unique ID of the monitor.

    **Success Response (`200 OK`)**
    ```json
    {
      "monitor": {
        "id": 1,
        "name": "Large ETH Transfers",
        "network": "ethereum",
        "address": null,
        "abi": null,
        "filter_script": "tx.value > ether(10)",
        "actions": [
          "my-webhook"
        ],
        "created_at": "2023-10-27T10:00:00Z",
        "updated_at": "2023-10-27T10:00:00Z"
      }
    }
    ```

    **Error Response (`404 Not Found`)**
    If no monitor with the specified ID exists.
    ```json
    {
      "error": "Monitor not found"
    }
    ```

    **Example Usage:**
    ```bash
    curl http://localhost:8080/monitors/1
    ```

