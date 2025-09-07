//! Provides a simple ABI JSON for testing purposes.

/// This ABI includes a function and an event to facilitate testing of
/// ABI-related functionalities.
pub fn simple_abi_json() -> &'static str {
    r#"[
        {
            "type": "function",
            "name": "transfer",
            "inputs": [
                {"name": "to", "type": "address"},
                {"name": "amount", "type": "uint256"}
            ],
            "outputs": [{"name": "success", "type": "bool"}]
        },
        {
            "type": "event",
            "name": "Transfer",
            "inputs": [
                {"name": "from", "type": "address", "indexed": true},
                {"name": "to", "type": "address", "indexed": true},
                {"name": "amount", "type": "uint256", "indexed": false}
            ],
            "anonymous": false
        }
    ]"#
}
