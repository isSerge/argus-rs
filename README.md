# Argus EVM Monitor

Argus is a next-generation, open-source, self-hosted monitoring tool for EVM chains

## Project Setup

### Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) (latest stable version)
- [sqlx-cli](https://github.com/launchbadge/sqlx/tree/main/sqlx-cli) for database migrations.

### Installation

1.  **Clone the repository:**
    ```bash
    git clone https://github.com/isSerge/argus-rs
    cd argus-rs
    ```

2.  **Install `sqlx-cli`:**
    ```bash
    cargo install sqlx-cli
    ```

## Configuration

The application is configured via a `config.yaml` file in the project root.

Example `config.yaml` file:
```yaml
database_url: "sqlite://monitor.db"
rpc_urls:
  - "https://mainnet.infura.io/v3/YOUR_INFURA_PROJECT_ID"
  - "https://eth-mainnet.alchemyapi.io/v2/YOUR_ALCHEMY_API_KEY"
network_id: "mainnet"
```

- `database_url`: The connection string for the SQLite database.
- `rpc_urls`: A list of RPC endpoint URLs for the EVM network. The application currently uses the first URL in the list.
- `network_id`: A unique identifier for the network being monitored (e.g., "mainnet", "sepolia").

## Database Setup

The application uses `sqlx` to manage database migrations. The state is stored in a local SQLite database file, configured via the `database_url` in `config.yaml`.

The database file will be created automatically on the first run if it doesn't exist.

1.  **Ensure `database_url` is set** in your `config.yaml` (see Configuration section).

2.  **Run migrations:**
    This command will create the necessary tables in the database. Note that `sqlx-cli` typically reads the `DATABASE_URL` from an environment variable. You may need to set it explicitly for the command, e.g., `DATABASE_URL="sqlite:monitor.db" sqlx migrate run`.
    ```bash
    sqlx migrate run
    ```

## Running the Application

Once the setup is complete, you can run the application.

1.  **Build the project:**
    ```bash
    cargo build --release
    ```

2.  **Run the application:**
    ```bash
    cargo run --release
    ```
