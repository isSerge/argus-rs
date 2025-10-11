# Performance Benchmarking

This directory contains scripts and configurations for running performance benchmarks to prevent regressions and measure improvements.

## Prerequisites

You will need the following tools installed:

-   **[`hyperfine`](https://github.com/sharkdp/hyperfine)**: A command-line benchmarking tool.
-   **[`erpc`](https://github.com/erpc/erpc)**: A high-performance Ethereum RPC used for caching.

## Workflow

The benchmarking process is designed to be consistent and repeatable, using a local RPC cache to eliminate network latency as a variable.

### 1. Start the Local RPC Cache

Before running any benchmarks, start a local `erpc` instance using Docker. It will use the `erpc.yaml` file in the root of the project for its configuration.

First, edit the `erpc.yaml` file and replace the placeholder `endpoint` with your actual Ethereum mainnet RPC URL.

Then, from the project's root directory, run the following command to start `erpc`:

```bash
docker run -v $(pwd)/benchmarks/erpc.yaml:/erpc.yaml -p 4000:4000 -p 4001:4001 --rm ghcr.io/erpc/erpc:latest &
```

Once running, ensure the `rpc_urls` in your benchmark's `app.yaml` is pointed to your local `erpc` instance (e.g., `http://127.0.0.1:4000/main/evm/1`).

### 2. Build the Application

Ensure you have a release build of the `argus` binary:

```bash
cargo build --release
```

### 3. Run the Benchmark

Use `hyperfine` to execute the `dry-run` command against a specific scenario. The `--config-dir` should point to the scenario's directory (e.g., `benchmarks/scenario_a_baseline_throughput`).

The first run will warm up the `anvil` cache. `hyperfine` automatically handles warm-up runs.

**Example for Scenario A:**

```bash
# This command runs the benchmark for blocks 23,545,500 to 23,545,600
hyperfine --warmup 1 './target/release/argus dry-run --from 23545500 --to 23545600 --config-dir benchmarks/scenario_a_baseline_throughput'
```

### 4. Comparing Results

To compare performance before and after a code change, export the results to JSON files.

1.  **Before Changes**:
    ```bash
    hyperfine --export-json benchmarks/scenario_a_before.json './target/release/argus dry-run --from 18000000 --to 18010000 --config-dir benchmarks/scenario_a_baseline_throughput'
    ```

2.  **After Changes**:
    ```bash
    hyperfine --export-json benchmarks/scenario_a_after.json './target/release/argus dry-run --from 18000000 --to 18010000 --config-dir benchmarks/scenario_a_baseline_throughput'
    ```

You can then use `hyperfine`'s built-in comparison feature:

```bash
hyperfine benchmarks/scenario_a_before.json benchmarks/scenario_a_after.json
```

## Scenarios

### Scenario A: Baseline Throughput

-   **Directory**: `scenario_a_baseline_throughput/`
-   **Objective**: Measure raw block ingestion and simple transaction filtering speed.
-   **Monitor Config**: A simple, low-cost filter (`tx.value > ether(1000)`).
