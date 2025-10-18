# ABI Management - Architecture & Flow Diagrams

## System Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                         Client Application                       │
│                    (curl, web app, etc.)                         │
└───────────────────────────────┬─────────────────────────────────┘
                                │
                    ┌───────────▼──────────┐
                    │  HTTP Request        │
                    │  + Bearer Token      │
                    └───────────┬──────────┘
                                │
┌───────────────────────────────▼─────────────────────────────────┐
│                       HTTP Server Layer                          │
│  ┌────────────────────────────────────────────────────────┐    │
│  │  Authentication Middleware (auth.rs)                    │    │
│  │  - Validates Bearer token for POST/DELETE               │    │
│  └────────────────────────────────────────────────────────┘    │
│  ┌────────────────────────────────────────────────────────┐    │
│  │  ABI Handlers (abis.rs)                                 │    │
│  │  - create_abi()      → POST /abis                       │    │
│  │  - get_abi_by_name() → GET /abis/{name}                │    │
│  │  - list_abi_names()  → GET /abis                        │    │
│  │  - get_abis()        → GET /abis/all                    │    │
│  │  - delete_abi()      → DELETE /abis/{name}             │    │
│  └────────────────────────────────────────────────────────┘    │
│  ┌────────────────────────────────────────────────────────┐    │
│  │  Error Handler (error.rs)                               │    │
│  │  - Maps PersistenceError to HTTP status codes           │    │
│  │  - Formats error responses                              │    │
│  └────────────────────────────────────────────────────────┘    │
└───────────────────────────────┬─────────────────────────────────┘
                                │
┌───────────────────────────────▼─────────────────────────────────┐
│                    Persistence Layer                             │
│  ┌────────────────────────────────────────────────────────┐    │
│  │  AppRepository Trait (traits.rs)                        │    │
│  │  - get_abis()                                           │    │
│  │  - get_abi_by_name(name)                               │    │
│  │  - add_abi(name, content)                              │    │
│  │  - delete_abi(name)                                    │    │
│  │  - is_abi_in_use(name)                                 │    │
│  └────────────────────────────────────────────────────────┘    │
│  ┌────────────────────────────────────────────────────────┐    │
│  │  SqliteStateRepository (app_repository.rs)              │    │
│  │  - Implements AppRepository trait                       │    │
│  │  - Handles database queries                             │    │
│  │  - Performs validation                                  │    │
│  │  - Manages transactions                                 │    │
│  └────────────────────────────────────────────────────────┘    │
└───────────────────────────────┬─────────────────────────────────┘
                                │
┌───────────────────────────────▼─────────────────────────────────┐
│                      SQLite Database                             │
│  ┌────────────────────────────────────────────────────────┐    │
│  │  abis Table                                             │    │
│  │  ┌──────────────┬──────────────┬─────────────────┐    │    │
│  │  │ name (PK)    │ abi_content  │ created_at      │    │    │
│  │  ├──────────────┼──────────────┼─────────────────┤    │    │
│  │  │ "erc20"      │ "[{...}]"    │ 2024-01-15...   │    │    │
│  │  │ "erc721"     │ "[{...}]"    │ 2024-01-16...   │    │    │
│  │  │ "weth"       │ "[{...}]"    │ 2024-01-17...   │    │    │
│  │  └──────────────┴──────────────┴─────────────────┘    │    │
│  └────────────────────────────────────────────────────────┘    │
│  ┌────────────────────────────────────────────────────────┐    │
│  │  monitors Table                                         │    │
│  │  ┌────┬──────┬─────────┬─────┬────────────────────┐   │    │
│  │  │ id │ name │ address │ abi │ filter_script      │   │    │
│  │  ├────┼──────┼─────────┼─────┼────────────────────┤   │    │
│  │  │ 1  │ "M1" │ "0x..." │"erc20"│ "log.name == ..." │   │    │
│  │  │ 2  │ "M2" │ "0x..." │"weth" │ "tx.value > ..."  │   │    │
│  │  └────┴──────┴─────────┴─────┴────────────────────┘   │    │
│  │                         │                               │    │
│  │                         └─ References abis.name         │    │
│  └────────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────────┘
```

## Request Flow - Upload ABI

```
Client                 HTTP Server           Persistence         Database
  │                         │                      │                 │
  ├─ POST /abis ────────────►                     │                 │
  │  + Auth Token           │                     │                 │
  │  + JSON Body            │                     │                 │
  │                         │                     │                 │
  │                         ├─ Validate Token ───►│                 │
  │                         │                     │                 │
  │                         ├─ Parse JSON         │                 │
  │                         │                     │                 │
  │                         ├─ Validate ABI name  │                 │
  │                         │                     │                 │
  │                         ├─ add_abi() ─────────►                 │
  │                         │                     │                 │
  │                         │                     ├─ Check duplicate►
  │                         │                     │                 │
  │                         │                     ├─ Validate JSON ─►
  │                         │                     │                 │
  │                         │                     ├─ INSERT ────────►
  │                         │                     │                 │
  │                         │                     ◄─ Success ───────┤
  │                         │                     │                 │
  │                         ◄─ Result ────────────┤                 │
  │                         │                     │                 │
  ◄─ 201 Created ───────────┤                     │                 │
  │  {"name": "erc20"}      │                     │                 │
```

## Request Flow - Delete ABI

```
Client                 HTTP Server           Persistence         Database
  │                         │                      │                 │
  ├─ DELETE /abis/erc20 ────►                     │                 │
  │  + Auth Token           │                     │                 │
  │                         │                     │                 │
  │                         ├─ Validate Token ───►│                 │
  │                         │                     │                 │
  │                         ├─ delete_abi() ──────►                 │
  │                         │                     │                 │
  │                         │                     ├─ is_abi_in_use()►
  │                         │                     │                 │
  │                         │                     ├─ Check monitors►│
  │                         │                     │  WHERE abi='erc20'
  │                         │                     │                 │
  │                         │   ┌─── In Use? ────◄─────────────────┤
  │                         │   │                 │                 │
  │                         │   ▼                 │                 │
  │                ┌────────┴────────┐            │                 │
  │                │ Yes: Return     │            │                 │
  │                │ Error 400       │            │                 │
  │                └────────┬────────┘            │                 │
  │                         │   ▼                 │                 │
  │                         │   No: Continue      │                 │
  │                         │                     │                 │
  │                         │                     ├─ DELETE ────────►
  │                         │                     │                 │
  │                         │                     ◄─ Success ───────┤
  │                         │                     │                 │
  │                         ◄─ Result ────────────┤                 │
  │                         │                     │                 │
  ◄─ 204 No Content ────────┤                     │                 │
```

## Data Flow - ABI Usage Protection

```
┌────────────────────────────────────────────────────────────┐
│                     Delete ABI Request                      │
│                    DELETE /abis/erc20                       │
└────────────────────────┬───────────────────────────────────┘
                         │
                         ▼
            ┌────────────────────────┐
            │  is_abi_in_use()?      │
            │  Query monitors table  │
            └───────┬────────────────┘
                    │
        ┌───────────▼──────────┐
        │ SELECT COUNT(*)      │
        │ FROM monitors        │
        │ WHERE abi = 'erc20'  │
        └───────┬──────────────┘
                │
    ┌───────────▼──────────────┐
    │   Count > 0?             │
    └───┬──────────────┬───────┘
        │              │
    YES │              │ NO
        │              │
        ▼              ▼
┌──────────────┐  ┌──────────────┐
│ Return Error │  │ DELETE ABI   │
│ 400 Bad Req  │  │ Return 204   │
└──────────────┘  └──────────────┘
```

## Error Handling Flow

```
                    ┌─────────────────────┐
                    │ Persistence Layer   │
                    │ Operation           │
                    └──────────┬──────────┘
                               │
                    ┌──────────▼──────────┐
                    │  Error Occurred?    │
                    └───┬──────────────┬──┘
                        │              │
                     YES│              │NO
                        │              │
            ┌───────────▼──────┐       ▼
            │ PersistenceError │   Success
            │                  │       │
            │ • AlreadyExists  │       │
            │ • NotFound       │       │
            │ • InvalidInput   │       │
            │ • OperationFailed│       │
            └───────────┬──────┘       │
                        │              │
            ┌───────────▼──────────────▼─────┐
            │   From<PersistenceError>       │
            │   Conversion                   │
            └───────────┬────────────────────┘
                        │
            ┌───────────▼──────────┐
            │      ApiError        │
            │                      │
            │ • Conflict (409)     │
            │ • NotFound (404)     │
            │ • BadRequest (400)   │
            │ • InternalError(500) │
            └───────────┬──────────┘
                        │
            ┌───────────▼──────────┐
            │  IntoResponse        │
            │  (HTTP Response)     │
            └───────────┬──────────┘
                        │
                        ▼
            ┌────────────────────────┐
            │ JSON Response          │
            │ {                      │
            │   "error": "message"   │
            │ }                      │
            └────────────────────────┘
```

## Database Schema Relationships

```
┌────────────────────────────────┐
│         abis table             │
├────────────────────────────────┤
│ name (PK) │ abi_content        │
├───────────┼────────────────────┤
│ "erc20"   │ "[{...}]"          │
│ "erc721"  │ "[{...}]"          │
│ "weth"    │ "[{...}]"          │
└──────▲────┴────────────────────┘
       │
       │ References (application-level)
       │ Foreign Key Check
       │
┌──────┴─────────────────────────┐
│      monitors table            │
├────────────────────────────────┤
│ id│name│address│abi│filter... │
├───┼────┼───────┼───┼──────────┤
│ 1 │"M1"│"0x.." │"erc20"│"..."│
│ 2 │"M2"│"0x.." │"weth" │"..."│
│ 3 │"M3"│"0x.." │"erc20"│"..."│
└────┴────┴───────┴───┴──────────┘
                  │
                  └─ Cannot delete "erc20"
                     because it's used by M1 & M3
```

## Component Dependencies

```
┌──────────────────────────────────────────────────────────┐
│                      Application                          │
│                                                            │
│  ┌─────────────────────────────────────────────────┐    │
│  │              HTTP Server Module                  │    │
│  │                                                   │    │
│  │  ┌──────────┐  ┌──────────┐  ┌──────────┐      │    │
│  │  │  mod.rs  │  │ abis.rs  │  │ error.rs │      │    │
│  │  └────┬─────┘  └────┬─────┘  └────┬─────┘      │    │
│  │       │             │              │             │    │
│  │       └─────────────┴──────────────┘             │    │
│  └──────────────────────┬───────────────────────────┘    │
│                         │ uses                            │
│  ┌──────────────────────▼───────────────────────────┐    │
│  │           Persistence Module                      │    │
│  │                                                   │    │
│  │  ┌──────────┐  ┌───────────────┐  ┌─────────┐  │    │
│  │  │traits.rs │  │app_repository │  │error.rs │  │    │
│  │  └────┬─────┘  │     .rs       │  └─────────┘  │    │
│  │       │        └───────┬───────┘                │    │
│  │       │ defines        │ implements             │    │
│  │       │                │                         │    │
│  │       └────────────────┘                         │    │
│  └──────────────────────┬───────────────────────────┘    │
│                         │ uses                            │
│  ┌──────────────────────▼───────────────────────────┐    │
│  │             Models Module                         │    │
│  │                                                   │    │
│  │  ┌──────────┐  ┌──────────┐  ┌──────────┐      │    │
│  │  │  mod.rs  │  │  abi.rs  │  │monitor.rs│      │    │
│  │  └──────────┘  └──────────┘  └──────────┘      │    │
│  └───────────────────────────────────────────────────    │
│                                                            │
└──────────────────────────────────────────────────────────┘
```

## State Transitions

```
ABI Lifecycle:
───────────────

  ┌─────────┐
  │ Created │ ◄─── POST /abis
  └────┬────┘
       │
       ▼
  ┌────────────┐
  │  Stored    │
  │ (Database) │
  └────┬───┬───┘
       │   │
       │   └──────────────────┐
       │                      │
       ▼                      ▼
  ┌─────────┐          ┌──────────┐
  │  In Use │          │ Not Used │
  │(Referenced        │          │
  │by monitors)       │          │
  └────┬────┘          └────┬─────┘
       │                    │
       │ Cannot             │ Can
       │ Delete             │ Delete
       │                    │
       ▼                    ▼
  ┌─────────┐          ┌─────────┐
  │Protected│          │ Deleted │
  │(400 Err)│          │(204 OK) │
  └─────────┘          └─────────┘
```

## Summary

These diagrams illustrate:
1. **Architecture**: How components interact
2. **Request Flow**: Sequence of operations
3. **Data Flow**: How data moves through the system
4. **Error Handling**: Error propagation and transformation
5. **Relationships**: Database schema connections
6. **Dependencies**: Module relationships
7. **Lifecycle**: State transitions for ABIs

The implementation provides a clean, layered architecture with clear separation of concerns and robust error handling.
