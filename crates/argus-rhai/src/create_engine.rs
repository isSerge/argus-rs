use argus_core::config::RhaiConfig;
use rhai::{Engine, packages::Package};
use rhai_bigint::BigIntPackage;
use rhai_evm::EvmPackage;

use super::proxies::register_proxies;

/// Creates a Rhai engine with security features and custom configurations.
/// Used for both RhaiCompiler (AST compilation) and RhaiFilteringEngine (AST
/// evaluation).
pub fn create_engine(rhai_config: RhaiConfig) -> Engine {
    let mut engine = Engine::new();

    // Apply security limits
    engine.set_max_operations(rhai_config.max_operations);
    engine.set_max_call_levels(rhai_config.max_call_levels);
    engine.set_max_string_size(rhai_config.max_string_size);
    engine.set_max_array_size(rhai_config.max_array_size);

    // Disable dangerous language features
    const DANGEROUS_SYMBOLS: &[&str] = &[
        "eval", "import", "export", "print", "debug", "File", "file", "http", "net", "system",
        "process", "thread", "spawn",
    ];
    for &symbol in DANGEROUS_SYMBOLS {
        engine.disable_symbol(symbol);
    }

    // Register BigInt package for handling large integers in token values
    BigIntPackage::new().register_into_engine(&mut engine);
    // Register EVM wrappers for handling decoded logs and calls
    EvmPackage::new().register_into_engine(&mut engine);

    // Register custom proxies for accessing decoded logs and calls
    register_proxies(&mut engine);

    engine
}
