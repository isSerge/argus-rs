# Installation

To get Argus up and running, you'll need to compile it from the source. This guide assumes you have a basic understanding of the command line and have the Rust toolchain installed.

## Prerequisites

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

## Compiling from Source

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
