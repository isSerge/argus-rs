# `app.yaml` Configuration

The `app.yaml` file is the heart of the Argus application's configuration. It defines the global settings for the service, including how it connects to the blockchain, how it stores data, and how it behaves under load.

## Example `app.yaml`

```yaml
# The connection string for the SQLite database.
database_url: "sqlite:monitor.db"

# A list of RPC endpoint URLs. Argus will cycle through these if one fails.
rpc_urls:
  - "https://eth.llamarpc.com"
  - "https://1rpc.io/eth"
  - "https://rpc.mevblocker.io"

# A unique identifier for the network being monitored.
network_id: "ethereum"

# The directory where contract ABI JSON files are located.
abi_config_path: abis/

# Performance and reliability settings.
block_chunk_size: 5
polling_interval_ms: 10000
confirmation_blocks: 12
```

## Configuration Parameters

### Core Settings

| Parameter | Description | Default |
| :--- | :--- | :--- |
| `database_url` | The connection string for the SQLite database. **This field is required.** | (none) |
| `rpc_urls` | A list of RPC endpoint URLs for the EVM network. Argus will use them in a fallback sequence if one fails. **At least one URL is required.** | (none) |
| `network_id` | A unique identifier for the network being monitored (e.g., "ethereum", "sepolia"). **This field is required.** | (none) |
| `abi_config_path` | The directory where contract ABI JSON files are located. | `abis/` |

### Performance & Reliability

| Parameter | Description | Default |
| :--- | :--- | :--- |
| `block_chunk_size` | The number of blocks to fetch and process in a single batch. | `5` |
| `polling_interval_ms` | The interval in milliseconds to poll for new blocks. | `10000` |
| `confirmation_blocks` | Number of blocks to wait for before processing to protect against reorgs. A higher number is safer but introduces more latency. | `12` |
| `notification_channel_capacity` | The capacity of the internal channel for sending notifications. | `1024` |
| `shutdown_timeout` | The maximum time in seconds to wait for a graceful shutdown. | `30` |
| `aggregation_check_interval` | The interval in seconds to check for aggregated matches for notifiers with policies. | `5` |

---

### Nested Configuration Sections

The following configurations are nested under their respective top-level keys in `app.yaml`.

### RPC Client Settings (`rpc_retry_config`)

These settings control the behavior of the client used to communicate with the EVM RPC endpoints.

**Default Configuration:**
```yaml
rpc_retry_config:
  max_retry: 10
  backoff_ms: 1000
  compute_units_per_second: 100
```

| Parameter | Description |
| :--- | :--- |
| `max_retry` | The maximum number of retries for a failing RPC request. |
| `backoff_ms` | The initial backoff delay in milliseconds for RPC retries. |
| `compute_units_per_second` | The number of compute units per second to allow (for rate limiting). |

### HTTP Client Settings (`http_retry_config`)

These settings control the retry behavior of the internal HTTP client, which is used for sending webhook notifications.

**Default Configuration:**
```yaml
http_retry_config:
  max_retries: 3
  initial_backoff_ms: 250
  max_backoff_secs: 10
  base_for_backoff: 2
  jitter: full
```

| Parameter | Description |
| :--- | :--- |
| `max_retries` | The maximum number of retries for a failing HTTP request. |
| `initial_backoff_ms` | The initial backoff delay in milliseconds for HTTP retries. |
| `max_backoff_secs` | The maximum backoff delay in seconds for HTTP retries. |
| `base_for_backoff` | The base for the exponential backoff calculation. |
| `jitter` | The jitter to apply to the backoff (`none` or `full`). |

### Rhai Script Engine Settings (`rhai`)

These settings provide guardrails for the Rhai scripts to prevent long-running or resource-intensive scripts from impacting the application's performance.

**Default Configuration:**
```yaml
rhai:
  max_operations: 100000
  max_call_levels: 10
  max_string_size: 8192
  max_array_size: 1000
  execution_timeout: 5000
```

| Parameter | Description |
| :--- | :--- |
| `max_operations` | Maximum number of operations a script can perform. |
| `max_call_levels` | Maximum function call nesting depth in a script. |
| `max_string_size` | Maximum size of strings in characters. |
| `max_array_size` | Maximum number of array elements. |
| `execution_timeout` | Maximum execution time per script in milliseconds. |
