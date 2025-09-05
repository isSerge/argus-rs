# Architecture

This document provides a high-level overview of the internal architecture of the Argus application.

## Core Principles

-   **Modular**: Each component has a distinct and well-defined responsibility.
-   **Asynchronous**: Built on top of Tokio, the application is designed to be highly concurrent and non-blocking.
-   **Stateful**: The application's progress is persisted to a local database, allowing for resilience and crash recovery.
-   **Decoupled**: Components communicate through channels, reducing tight coupling and improving maintainability.

## High-Level Diagram

<!-- The Mermaid diagram was removed due to rendering issues in the current environment. It will be re-added once the rendering is supported. -->


## Key Components

The `src` directory is organized into several modules, each representing a key component of the system.

-   **`supervisor`**: The top-level orchestrator. It is responsible for initializing all other components, wiring them together, and managing the graceful shutdown of the application.

-   **`providers` (Block Ingestor)**: This component is responsible for fetching block data from the external EVM RPC nodes. It handles connection management, retries, and polling for new blocks. It keeps track of the last processed block number (stored in the database) to know where to resume from.

-   **`engine` (Block Processor)**: This is the core filtering logic of the application. It receives new blocks from the Block Ingestor, iterates through transactions and logs, and executes the appropriate Rhai filter scripts for each configured monitor.

-   **`notification` (Alert Manager)**: This component receives "matches" from the engine when a filter script returns `true`. It is responsible for managing the notification policies (throttling, aggregation) and dispatching the final alerts to the appropriate external services (e.g., webhooks, Slack).

-   **`persistence`**: This module provides an abstraction layer over the database (currently SQLite). It handles all state management, such as storing the last processed block, monitor configurations, and notifier policies.

-   **`config` & `loader`**: These modules manage the loading, parsing, and validation of the application's configuration from the YAML files.

-   **`models`**: Defines the core data structures used throughout the application (e.g., `BlockData`, `Transaction`, `Log`, `Monitor`, `Notifier`).

-   **`http_client`**: Provides a robust and reusable HTTP client with built-in retry logic, used by the notification component to send alerts.

-   **`main.rs`**: The application's entry point. It handles command-line argument parsing and kicks off the supervisor.
