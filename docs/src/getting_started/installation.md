# Installation

There are two primary ways to install and run Argus: using Docker (recommended for most users) or by building from source (ideal for developers and contributors).

## Using Docker (Recommended)

The quickest and most reliable way to get Argus running is with Docker and Docker Compose. This approach isolates dependencies and simplifies the setup process.

### Prerequisites

1.  **Docker**: Install [Docker Engine and Docker Compose](https://docs.docker.com/get-docker/).
2.  **Git**: To clone the repository.

### Setup

1.  **Clone the repository:**
    ```bash
    git clone https://github.com/isSerge/argus-rs
    cd argus-rs
    ```

2.  **Configure Secrets:**
    Argus uses a `.env` file to manage secrets for notifiers. Copy the example file to create your own.
    ```bash
    cp .env.example .env
    ```
    Now, open the `.env` file and fill in the required tokens and webhook URLs for the notifiers you plan to use.

3.  **Create Data Directory:**
    The Docker Compose setup is configured to persist the application's database in a local `data` directory.
    ```bash
    mkdir -p data
    ```

With these steps complete, you are ready to run the application. See the [Quick Start](./quick_start.md) guide for instructions on how to run your first monitor.

---

## Building from Source (for local development)

This method is for users who want to build the project from source code, for example, to contribute to development or to run it without Docker.

### Prerequisites

Before you begin, ensure you have the following tools installed on your system:

1.  **Rust**: Argus is built in Rust. If you don't have the Rust toolchain installed, you can get it from the official [rust-lang.org](https://www.rust-lang.org/tools/install) website. The standard installation via `rustup` is recommended.

    ```bash
    # Example installation command (see website for latest)
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
    ```

2.  **Git**: You will need Git to clone the repository.

3.  **`sqlx-cli`**: Argus uses `sqlx` for database migrations. You'll need to install the companion CLI tool to prepare the database. You can install it using `cargo`:

    ```bash
    cargo install sqlx-cli
    ```

### Compiling

Once the prerequisites are in place, you can clone the repository and build the project.

1.  **Clone the repository:**

    ```bash
    git clone https://github.com/isSerge/argus-rs
    cd argus-rs
    ```

2.  **Build the project in release mode:**

    This command compiles an optimized binary for production use.

    ```bash
    cargo build --release
    ```

    The final binary will be located at `target/release/argus`.

## Next Steps

With Argus successfully compiled, the next step is to set up your configuration files and prepare the database. This is covered in the [Quick Start](./quick_start.md) guide.