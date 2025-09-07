use std::{fs, sync::Arc};

use tempfile::TempDir;

use crate::abi::{AbiRepository, AbiService};

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

pub fn create_test_abi_service(
    temp_dir: &TempDir,
    abis: &[(&str, &str)],
) -> (Arc<AbiService>, Arc<AbiRepository>) {
    for (name, content) in abis {
        let file_path = temp_dir.path().join(format!("{}.json", name));
        fs::write(file_path, *content).unwrap();
    }

    let abi_repo = Arc::new(AbiRepository::new(temp_dir.path()).unwrap());
    let abi_service = Arc::new(AbiService::new(abi_repo.clone()));
    (abi_service, abi_repo)
}
