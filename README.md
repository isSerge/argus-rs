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

The application is configured via environment variables. You can place these in a `.env` file in the project root for development.

- `DATABASE_URL`: The connection string for the SQLite database.

Example `.env` file:
```
DATABASE_URL=sqlite:monitor.db
```

## Database Setup

The application uses `sqlx` to manage database migrations. The state is stored in a local SQLite database file, configured via the `DATABASE_URL` environment variable.

The database file will be created automatically on the first run if it doesn't exist.

1.  **Create a `.env` file** with the `DATABASE_URL` (see Configuration section).

2.  **Run migrations:**
    This command will create the necessary tables in the database. `sqlx-cli` automatically reads the `.env` file.
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
