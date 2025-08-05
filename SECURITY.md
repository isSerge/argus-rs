# Security Features

Argus implements security limits for Rhai script execution to protect against resource exhaustion and malicious scripts.

## Configuration

Security settings are configured in `config.yaml`:

```yaml
rhai:
  max_operations: 100000        # Maximum operations per script
  max_call_levels: 10          # Maximum function nesting depth
  max_string_size: 8192        # Maximum string size in characters
  max_array_size: 1000         # Maximum array elements
  execution_timeout: 5000      # Maximum execution time in milliseconds
```

## Security Limits

- **Operation Limits**: Prevents infinite loops and excessive computations
- **Call Depth Limits**: Protects against stack overflow from recursion
- **String/Array Size Limits**: Prevents memory exhaustion
- **Execution Timeouts**: Prevents scripts from running indefinitely
- **Graceful Error Handling**: Script failures don't affect system stability

All limits are enforced at runtime and errors are handled gracefully without affecting system operation.
