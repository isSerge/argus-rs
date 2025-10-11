# Performance Benchmarking

This directory contains scripts and configurations for running performance benchmarks to prevent regressions and measure improvements.

## Common Setup

Follow these steps before running any benchmark scenario.

### 1. Prerequisites

You will need the following tools installed:

-   **[`hyperfine`](https://github.com/sharkdp/hyperfine)**: A command-line benchmarking tool.
-   **[`docker`](https://www.docker.com/)**: To run the local `erpc` caching instance.

### 2. Start the Local RPC Cache

Before running any benchmarks, start a local `erpc` instance using Docker. It will use the `erpc.yaml` file in this directory for its configuration.

First, edit `benchmarks/erpc.yaml` and replace the placeholder `endpoint` with your actual Ethereum mainnet RPC URL.

Then, from the **project's root directory**, run the following command to start `erpc`:

```bash
docker run -v $(pwd)/benchmarks/erpc.yaml:/erpc.yaml -p 4000:4000 -p 4001:4001 --rm ghcr.io/erpc/erpc:latest &
```

The first run of any benchmark will warm up the `erpc` cache. `hyperfine` automatically handles warm-up runs.

### 3. Build the Application

Ensure you have a release build of the `argus` binary:

```bash
cargo build --release
```

## Benchmark Scenarios

Run the command for the scenario you wish to benchmark. The block range `23,545,500` to `23,545,600` is used as a consistent sample.

### Scenario A: Baseline Throughput

-   **Objective**: Measure raw block ingestion and simple transaction filtering speed.

```bash
hyperfine --warmup 1 './target/release/argus dry-run --from 23545500 --to 23545600 --config-dir benchmarks/scenario_a_baseline_throughput'
```

### Scenario B: Log-Heavy Workload

-   **Objective**: Stress the event log decoding and matching logic.

```bash
hyperfine --warmup 1 './target/release/argus dry-run --from 23545500 --to 23545600 --config-dir benchmarks/scenario_b_log_heavy'
```

### Scenario C: Calldata-Heavy Workload

-   **Objective**: Stress the transaction input (calldata) decoding logic.

```bash
hyperfine --warmup 1 './target/release/argus dry-run --from 23545500 --to 23545600 --config-dir benchmarks/scenario_c_calldata_heavy'
```

## Comparing Results

To compare performance before and after a code change, export the results to JSON files.

1.  **Before Changes** (using Scenario A as an example):
    ```bash
    hyperfine --export-json benchmarks/scenario_a_before.json './target/release/argus dry-run --from 23545500 --to 23545600 --config-dir benchmarks/scenario_a_baseline_throughput'
    ```

2.  **After Changes**:
    ```bash
    hyperfine --export-json benchmarks/scenario_a_after.json './target/release/argus dry-run --from 23545500 --to 23545600 --config-dir benchmarks/scenario_a_baseline_throughput'
    ```

You can then use `hyperfine`'s built-in comparison feature:

```bash
hyperfine benchmarks/scenario_a_before.json benchmarks/scenario_a_after.json
```
