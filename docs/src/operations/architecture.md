# Architecture

This document provides a high-level overview of the internal architecture of the Argus application.

## Core Principles

-   **Modular**: Each component has a distinct and well-defined responsibility.
-   **Asynchronous**: Built on top of Tokio, the application is designed to be highly concurrent and non-blocking.
-   **Stateful**: The application's progress is persisted to a local database, allowing for resilience and crash recovery.
-   **Decoupled**: Components communicate through channels, reducing tight coupling and improving maintainability.

## Key Components

The `src` directory is organized into several modules, each representing a key component of the system.

-   **`supervisor`**: The top-level orchestrator. It is responsible for initializing all other components, wiring them together via channels, and managing the graceful shutdown of the application.

-   **`monitor`**: This module, centered around the `MonitorManager`, is responsible for the lifecycle of monitor configurations. It loads, validates, and analyzes the monitors, preparing them for the filtering engine. It handles dynamic updates to the monitor set.

-   **`providers`**: This component is responsible for fetching block data from the external EVM RPC nodes. It handles connection management, retries, and polling for new blocks.

-   **`engine`**: This is the core data processing pipeline. It is divided into two main stages:
    -   **`BlockProcessor`**: Receives raw block data from the providers and correlates transactions with their corresponding logs and receipts into a structured format.
    -   **`FilteringEngine`**: Receives correlated block data from the `BlockProcessor`. It executes the appropriate Rhai filter scripts for each monitor, lazily decoding event logs and transaction calldata as needed during script execution. Upon a match, it creates a `MonitorMatch` object.

-   **`engine/action_handler`**: Manages the execution of JavaScript actions that can modify `MonitorMatch` objects before they are sent to notifiers. When actions are configured, it communicates with the standalone `js_executor` process via Unix domain sockets.

-   **`notification` (Alert Manager)**: This component receives `MonitorMatch`es from the `FilteringEngine` (optionally after processing by the action handler). It is responsible for managing notification policies (throttling, aggregation) and dispatching the final alerts to external services (e.g., webhooks).

-   **`persistence`**: This module provides an abstraction layer over the database (currently SQLite). It handles all state management, such as storing the last processed block number.

-   **`config` & `loader`**: These modules manage the loading, parsing, and validation of the application's configuration from the YAML files.

-   **`models`**: Defines the core data structures used throughout the application (e.g., `BlockData`, `Transaction`, `Log`, `Monitor`, `Notifier`).

-   **`http_client`**: Provides a robust and reusable HTTP client with built-in retry logic, used by the notification component to send alerts.

-   **`main.rs`**: The application's entry point. It handles command-line argument parsing and kicks off the supervisor.

## JavaScript Executor (js_executor)

The `js_executor` is a standalone binary (located in `crates/js_executor/`) that provides JavaScript execution capabilities for action scripts. It runs as a separate process from the main Argus application.

### Key Features

-   **Standalone Process**: Runs independently with its own dependency tree, avoiding conflicts with the main application's dependencies
-   **Unix Socket Communication**: Communicates with the main Argus process via Unix domain sockets for high-performance IPC
-   **deno_core Integration**: Uses Deno's V8 runtime to execute JavaScript code securely
-   **Automatic Management**: The main Argus application automatically spawns and manages the js_executor process when actions are configured

### Communication Flow

1. When a monitor match occurs and actions are configured, the main Argus process sends an execution request to js_executor via Unix socket
2. js_executor receives the JavaScript code and match data, executes the script in a V8 isolate
3. The script can modify the match object, which is then returned to the main process
4. The modified match data continues through the notification pipeline

This architecture ensures that JavaScript execution is isolated from the main application while maintaining high performance through efficient IPC.
