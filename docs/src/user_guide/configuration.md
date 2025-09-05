# Configuration Overview

Argus is configured primarily through a set of YAML files. By default, the application looks for these files in a `configs/` directory in the current working directory. You can specify a different directory using the `--config-dir <path>` command-line argument.

The configuration is split into three key files:

1.  **`app.yaml`**: Contains global application settings, such as RPC endpoints, database connections, and performance tuning parameters. This is the main configuration for the Argus service itself.

2.  **`monitors.yaml`**: This is where you define *what* you want to monitor on the blockchain. Each monitor specifies a network, an optional contract address, and a Rhai filter script that determines if a transaction or log is a match.

3.  **`notifiers.yaml`**: This file defines *how* you want to be notified when a monitor finds a match. You can configure various notification channels (e.g., webhooks) and set policies like throttling.

This modular approach allows for a clean separation of concerns, making it easier to manage complex monitoring setups.

Select a topic below for a detailed breakdown of each file and its parameters.

-   [**`app.yaml` Configuration (`app_yaml.md`)**](./app_yaml.md)
-   [**Monitors Configuration (`monitors_yaml.md`)**](./monitors_yaml.md)
-   [**Notifiers Configuration (`notifiers_yaml.md`)**](./notifiers_yaml.md)
-   [**ABI Management (`config_abis.md`)**](./config_abis.md)
