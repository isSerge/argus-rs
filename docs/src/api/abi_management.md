# ABI Management API

This document describes the ABI management endpoints for the Argus monitoring system.

## Overview

The ABI management API allows you to dynamically manage Application Binary Interfaces (ABIs) at runtime. This enables you to add, retrieve, list, and delete ABIs without restarting the application.

## Authentication

Write operations (POST, DELETE) require authentication using a Bearer token. Include the API key in the request header:

```
Authorization: Bearer <your-api-key>
```

Read operations (GET) do not require authentication.

## Endpoints

### 1. Upload an ABI

Creates a new ABI in the system.

**Endpoint:** `POST /abis`

**Authentication:** Required

**Request Body:**
```json
{
  "name": "erc20",
  "abi": [
    {
      "type": "function",
      "name": "transfer",
      "inputs": [
        {"name": "_to", "type": "address"},
        {"name": "_value", "type": "uint256"}
      ],
      "outputs": [{"type": "bool"}]
    }
  ]
}
```

**Response:** `201 Created`
```json
{
  "name": "erc20"
}
```

**Error Responses:**
- `400 Bad Request`: Invalid ABI name or invalid JSON format
- `409 Conflict`: ABI with the same name already exists
- `401 Unauthorized`: Missing or invalid API key

**Notes:**
- ABI names can only contain alphanumeric characters, underscores, and hyphens
- The `abi` field must be a valid JSON array

### 2. Get an ABI

Retrieves a specific ABI by its name.

**Endpoint:** `GET /abis/{name}`

**Authentication:** Not required

**Response:** `200 OK`
```json
{
  "name": "erc20",
  "abi": [...],
  "created_at": "2024-01-15T10:30:00Z",
  "updated_at": "2024-01-15T10:30:00Z"
}
```

**Error Responses:**
- `404 Not Found`: ABI with the specified name does not exist

### 3. List ABI Names

Retrieves a list of all available ABI names.

**Endpoint:** `GET /abis`

**Authentication:** Not required

**Response:** `200 OK`
```json
{
  "names": ["erc20", "erc721", "aave_governance_v2", "weth"]
}
```

### 4. Get All ABIs

Retrieves all ABIs with their full content.

**Endpoint:** `GET /abis/all`

**Authentication:** Not required

**Response:** `200 OK`
```json
{
  "abis": [
    {
      "name": "erc20",
      "abi": [...],
      "created_at": "2024-01-15T10:30:00Z",
      "updated_at": "2024-01-15T10:30:00Z"
    },
    {
      "name": "erc721",
      "abi": [...],
      "created_at": "2024-01-16T14:20:00Z",
      "updated_at": "2024-01-16T14:20:00Z"
    }
  ]
}
```

### 5. Delete an ABI

Removes an ABI from the system.

**Endpoint:** `DELETE /abis/{name}`

**Authentication:** Required

**Response:** `204 No Content`

**Error Responses:**
- `400 Bad Request`: ABI is currently in use by one or more monitors
- `404 Not Found`: ABI with the specified name does not exist
- `401 Unauthorized`: Missing or invalid API key

**Notes:**
- An ABI cannot be deleted if it is currently referenced by any monitor
- You must first update or delete all monitors using the ABI before deletion

## Usage Examples

### Upload an ABI

```bash
curl -X POST http://localhost:8080/abis \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer your-api-key" \
  -d '{
    "name": "erc20",
    "abi": [{"type": "function", "name": "transfer"}]
  }'
```

### Get an ABI

```bash
curl http://localhost:8080/abis/erc20
```

### List All ABI Names

```bash
curl http://localhost:8080/abis
```

### Get All ABIs

```bash
curl http://localhost:8080/abis/all
```

### Delete an ABI

```bash
curl -X DELETE http://localhost:8080/abis/erc20 \
  -H "Authorization: Bearer your-api-key"
```

## Integration with Monitors

When creating or updating monitors, you can now reference ABIs by their name instead of including the full ABI JSON. The monitor will look up the ABI from the database at runtime.

Example monitor configuration:
```yaml
monitors:
  - name: "USDC Transfer Monitor"
    network: "mainnet"
    address: "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48"
    abi: "erc20"  # References the ABI by name
    filter_script: |
      log.name == "Transfer" && log.params.value > usdc(1_000_000)
```

## Best Practices

1. **Naming Convention**: Use descriptive, lowercase names with underscores or hyphens (e.g., `erc20`, `aave_governance_v2`)

2. **Validation**: Always validate your ABI JSON before uploading to avoid runtime errors

3. **Reusability**: Store commonly used ABIs (like ERC20, ERC721) centrally for reuse across multiple monitors

4. **Deletion Safety**: The system prevents deletion of ABIs that are in use, ensuring monitors continue to function

5. **API Keys**: Keep your API keys secure and rotate them regularly

## Error Handling

All error responses follow this format:
```json
{
  "error": "Error message description"
}
```

Common HTTP status codes:
- `200 OK`: Request succeeded
- `201 Created`: Resource created successfully
- `204 No Content`: Resource deleted successfully
- `400 Bad Request`: Invalid input
- `401 Unauthorized`: Authentication required or failed
- `404 Not Found`: Resource not found
- `409 Conflict`: Resource already exists
- `500 Internal Server Error`: Server error occurred
