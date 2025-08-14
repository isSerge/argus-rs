use rhai::Engine;

use crate::config::RhaiConfig;

/// Creates a Rhai engine with security features and custom configurations.
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

    // Register BigInt wrapper for transparent big number handling
    super::bigint::register_bigint_with_rhai(&mut engine);

    engine
}
