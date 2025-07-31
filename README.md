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

## Database Setup

The application uses `sqlx` to manage database migrations. The state is stored in a local SQLite database file.

1.  **Create the database file:**
    This command will create an empty `monitor.db` file in the project root.
    ```bash
    sqlx database create --database-url sqlite:monitor.db
    ```

2.  **Run migrations:**
    This command will create the necessary tables in the database.
    ```bash
    sqlx migrate run --database-url sqlite:monitor.db
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
