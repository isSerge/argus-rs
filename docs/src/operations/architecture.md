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

-   **`notification` (Alert Manager)**: This component receives `MonitorMatch`es from the `FilteringEngine`. It is responsible for managing notification policies (throttling, aggregation) and dispatching the final alerts to external services (e.g., webhooks).

-   **`persistence`**: This module provides an abstraction layer over the database (currently SQLite). It handles all state management, such as storing the last processed block number.

-   **`config` & `loader`**: These modules manage the loading, parsing, and validation of the application's configuration from the YAML files.

-   **`models`**: Defines the core data structures used throughout the application (e.g., `BlockData`, `Transaction`, `Log`, `Monitor`, `Notifier`).

-   **`http_client`**: Provides a robust and reusable HTTP client with built-in retry logic, used by the notification component to send alerts.

-   **`main.rs`**: The application's entry point. It handles command-line argument parsing and kicks off the supervisor.
