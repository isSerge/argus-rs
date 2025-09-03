# `app.yaml` Configuration

The `app.yaml` file is the heart of the Argus application's configuration. It defines the global settings for the service, including how it connects to the blockchain, how it stores data, and how it behaves under load.

## Example `app.yaml`

```yaml
database_url: "sqlite:monitor.db"
rpc_urls:
  - "https://eth.llamarpc.com"
  - "https://1rpc.io/eth"
  - "https://rpc.mevblocker.io"
network_id: "mainnet"
block_chunk_size: 5
polling_interval_ms: 10000
confirmation_blocks: 12
notification_channel_capacity: 1024
abi_config_path: abis/
```

## Configuration Parameters

### Core Settings

| Parameter | Description | Required | Default |
| :--- | :--- | :--- | :--- |
| `database_url` | The connection string for the SQLite database. | Yes | |
| `rpc_urls` | A list of RPC endpoint URLs for the EVM network. Argus will cycle through these if one fails. | Yes | |
| `network_id` | A unique identifier for the network being monitored (e.g., "mainnet", "sepolia"). | Yes | |
| `abi_config_path` | The directory where contract ABI JSON files are located. | No | `abis/` |

### Performance & Reliability

| Parameter | Description | Required | Default |
| :--- | :--- | :--- | :--- |
| `block_chunk_size` | The number of blocks to fetch and process in a single batch. | No | `5` |
| `polling_interval_ms` | The interval in milliseconds to poll for new blocks. | No | `10000` |
| `confirmation_blocks` | Number of blocks to wait for before processing to protect against reorgs. | No | `12` |
| `notification_channel_capacity` | The capacity of the internal channel for sending notifications. | No | `1024` |
| `shutdown_timeout_secs` | The maximum time in seconds to wait for a graceful shutdown. | No | `30` |
| `aggregation_check_interval_secs` | The interval in seconds to check for aggregated matches for notifiers with policies. | No | `5` |

### HTTP Client Settings

These settings control the behavior of the internal HTTP client, which is used for sending webhook notifications.

| Parameter | Description | Default |
| :--- | :--- | :--- |
| `http_base_config.max_idle_per_host` | Maximum number of idle connections per host. | `10` |
| `http_base_config.idle_timeout` | Timeout in seconds for idle HTTP connections. | `30` |
| `http_base_config.connect_timeout` | Timeout in seconds for establishing HTTP connections. | `10` |
| `http_retry_config.max_retry` | The maximum number of retries for a failing HTTP request. | `3` |
| `http_retry_config.initial_backoff_ms` | The initial backoff delay in milliseconds for HTTP retries. | `250` |
| `http_retry_config.max_backoff_secs` | The maximum backoff delay in seconds for HTTP retries. | `10` |
| `http_retry_config.base_for_backoff` | The base for the exponential backoff calculation. | `2` |
| `http_retry_config.jitter` | The jitter to apply to the backoff (`none` or `full`). | `full` |

### RPC Client Settings

These settings control the behavior of the client used to communicate with the EVM RPC endpoints.

| Parameter | Description | Default |
| :--- | :--- | :--- |
| `rpc_retry_config.max_retry` | The maximum number of retries for a failing RPC request. | `10` |
| `rpc_retry_config.backoff_ms` | The initial backoff delay in milliseconds for RPC retries. | `1000` |
| `rpc_retry_config.compute_units_per_second` | The number of compute units per second to allow (for rate limiting). | `100` |

### Rhai Script Engine Settings

These settings provide guardrails for the Rhai scripts to prevent long-running or resource-intensive scripts from impacting the application's performance.

| Parameter | Description | Default |
| :--- | :--- | :--- |
| `rhai.max_operations` | Maximum number of operations a script can perform. | `100000` |
| `rhai.max_call_levels` | Maximum function call nesting depth in a script. | `10` |
| `rhai.max_string_size` | Maximum size of strings in characters. | `8192` |
| `rhai.max_array_size` | Maximum number of array elements. | `1000` |
| `rhai.execution_timeout` | Maximum execution time per script in milliseconds. | `5000` |
