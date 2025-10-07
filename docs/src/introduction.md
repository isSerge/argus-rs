# Introduction

Welcome to the documentation for Argus, a powerful and flexible open-source monitoring tool for EVM-compatible blockchains.

Argus is designed to provide a sleepless, vigilant eye over on-chain activity, giving you the power to react to events in real-time. It serves as a critical piece of infrastructure for any project relying on or interacting with EVM chains.

## What is Argus?

Argus is a self-hosted service that connects to an EVM node, processes new blocks as they are mined, and evaluates transactions and event logs against your custom-defined rules. When a rule is matched, Argus can trigger notifications or other automated workflows.

It is built with a few core principles in mind:

*   **Reliability at the Core**: Built in Rust, Argus is designed for high-performance, concurrent, and safe operation, ensuring it can be a dependable part of your infrastructure.
*   **Deep Flexibility**: At the heart of Argus is the [Rhai](https://rhai.rs) scripting engine. This allows you to write expressive, powerful, and fine-grained filtering logic that goes far beyond simple "from/to" address matching. If you can express it in a script, you can monitor for it.
*   **API-First Design**: Argus includes a REST API, allowing you to dynamically inspect monitor configurations without downtime.
*   **Stateful and Resilient**: Argus tracks its progress in a local database, allowing it to gracefully handle restarts and resume monitoring exactly where it left off, ensuring no blocks are missed.

This documentation will guide you through installing Argus, configuring your first monitors, and mastering its powerful filtering capabilities.
