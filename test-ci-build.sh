#!/bin/bash
set -e

echo "🧪 Testing CI build process simulation..."

# Test debug builds (like CI)
echo "📦 Building workspace targets (debug)..."
cargo build --workspace --all-targets

echo "🚀 Building js_executor separately (debug)..."
cargo build --manifest-path crates/js_executor/Cargo.toml

echo "🔍 Verifying debug binaries exist..."
if [ -f "target/debug/argus" ]; then
    echo "✅ argus debug binary found"
else
    echo "❌ argus debug binary not found"
    exit 1
fi

if [ -f "target/debug/js_executor" ]; then
    echo "✅ js_executor debug binary found (workspace copy)"
else
    echo "❌ js_executor debug binary not found in workspace target"
    exit 1
fi

echo "🧪 Running debug integration test..."
JS_EXECUTOR_BIN_PATH="target/debug/js_executor" cargo test js_client::tests::test_submit_script_success --lib

# Test release builds (like Docker/examples)
echo "📦 Building workspace targets (release)..."
cargo build --workspace --release

echo "🚀 Building js_executor separately (release)..."
cargo build --release --manifest-path crates/js_executor/Cargo.toml

echo "🔍 Verifying release binaries exist..."
if [ -f "target/release/argus" ]; then
    echo "✅ argus release binary found"
else
    echo "❌ argus release binary not found"
    exit 1
fi

if [ -f "crates/js_executor/target/release/js_executor" ]; then
    echo "✅ js_executor release binary found (crate-local)"
else
    echo "❌ js_executor release binary not found"
    exit 1
fi

echo "🧪 Running release integration test..."
JS_EXECUTOR_BIN_PATH="crates/js_executor/target/release/js_executor" cargo test --release js_client::tests::test_submit_script_success --lib

echo "🎉 All CI simulation tests passed!"
