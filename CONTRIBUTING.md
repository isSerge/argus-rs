# Contributing to Argus

First off, thank you for considering contributing to Argus! This document provides some guidelines for contributing to Argus. Please feel free to propose changes to this document in a pull request.

## How Can I Contribute?

There are many ways to contribute, from writing code and documentation to reporting bugs and suggesting features.

### Reporting Bugs

If you find a bug, please ensure the bug was not already reported by searching on GitHub under [Issues](https://github.com/isSerge/argus-rs/issues).

If you're unable to find an open issue addressing the problem, [open a new one](https://github.com/isSerge/argus-rs/issues/new). Be sure to include a **title and clear description**, as much relevant information as possible, and a **code sample or an executable test case** demonstrating the expected behavior that is not occurring.

### Suggesting Enhancements

If you have an idea for a new feature or an improvement to an existing one, please open an issue with a clear title and description. Explain your suggestion and why it would be beneficial to the project.

## Your First Code Contribution

Unsure where to begin contributing to Argus? You can start by looking through `good first issue` and `help wanted` issues.

### Development Setup

1.  **Install Rust:** If you don't have it already, install the Rust toolchain using [rustup](https://rustup.rs/).
2.  **Clone the repository:** `git clone https://github.com/isSerge/argus-rs.git`
3.  **Build the project:** `cargo build`
4.  **You're ready to go!**

### Making Changes

1.  Create a new branch for your changes:
    ```sh
    git checkout -b my-feature-branch
    ```
2.  Make your changes, ensuring you add or update tests as appropriate.
3.  Ensure your code is well-formatted and free of common issues by running the following commands:
    ```sh
    # Format the code
    cargo fmt --all

    # Run Clippy to catch common mistakes
    cargo clippy --all -- -D warnings
    ```
4.  Run the test suite to ensure everything is still working correctly:
    ```sh
    cargo test
    ```
5.  Commit your changes with a clear and descriptive commit message.

### Performance Benchmarking

To prevent performance regressions, Argus includes a suite of benchmarks. If your changes could impact performance, please run the benchmarks and include the results in your pull request description.

For detailed instructions on how to run the benchmarks, see the [benchmarks/README.md](./benchmarks/README.md) file.

### Submitting a Pull Request

1.  Push your branch to your fork on GitHub.
2.  Open a pull request to the `main` branch of the Argus repository.
3.  Fill out the pull request template, providing a clear description of your changes and linking to any relevant issues.
4.  Ensure all CI checks pass.
5.  A maintainer will review your PR, provide feedback, and merge it once it's ready.
