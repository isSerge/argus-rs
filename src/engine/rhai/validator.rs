use std::{collections::HashSet, sync::Arc};

use alloy::json_abi::JsonAbi;
use thiserror::Error;

use super::{
    RhaiCompiler, RhaiCompilerError, ScriptAnalysis, get_valid_log_rhai_paths,
    get_valid_receipt_rhai_paths, get_valid_tx_rhai_paths,
};

#[derive(Clone)]
/// Validates Rhai scripts against allowed fields and ABI.
pub struct RhaiScriptValidator {
    compiler: Arc<RhaiCompiler>,
}

/// Errors that can occur during Rhai script validation.
#[derive(Debug, Error)]
pub enum RhaiScriptValidationError {
    /// The script contains an invalid field access.
    #[error("Invalid field access: '{0}'. The field does not exist in the data model.")]
    InvalidField(String),

    /// The script accesses log fields not present in the provided ABI.
    #[error("Invalid log field access: {0}")]
    InvalidAbiField(String),

    /// Error from the Rhai compiler during analysis.
    #[error("Rhai syntax error: {0}")]
    RhaiSyntaxError(#[from] RhaiCompilerError),

    /// The script does not return a boolean value.
    #[error("Script does not return a boolean value: {0}")]
    InvalidReturnType(String),

    /// The script is missing an ABI.
    #[error("Script accesses 'log' data, but no contract ABI was provided.")]
    MissingAbi,

    /// Error during Rhai script evaluation.
    #[error("Rhai evaluation error: {0}")]
    RhaiEvaluationError(#[from] Box<rhai::EvalAltResult>),
}

/// Result of Rhai script validation
#[derive(Debug, Clone)]
pub struct RhaiScriptValidationResult {
    pub ast_analysis: ScriptAnalysis,
    pub requires_receipt: bool,
}

impl RhaiScriptValidator {
    /// Creates a new Rhai script validator with the given compiler.
    pub fn new(compiler: Arc<RhaiCompiler>) -> Self {
        Self { compiler }
    }

    /// Validates a Rhai script against the provided ABI.
    pub fn validate_script(
        &self,
        script: &str,
        abi: Option<&JsonAbi>,
    ) -> Result<RhaiScriptValidationResult, RhaiScriptValidationError> {
        // Compile and analyze script using compiler
        let ast_analysis = self.compiler.analyze_script(script)?;

        // Validate AST analysis
        self.validate_analysis(ast_analysis, abi)
    }

    /// Validates the AST analysis against allowed fields and ABI.
    fn validate_analysis(
        &self,
        ast_analysis: ScriptAnalysis,
        abi: Option<&JsonAbi>,
    ) -> Result<RhaiScriptValidationResult, RhaiScriptValidationError> {
        // Create a set of valid STATIC fields
        let mut valid_static_fields = get_valid_tx_rhai_paths();
        valid_static_fields.extend(get_valid_receipt_rhai_paths());
        valid_static_fields.extend(get_valid_log_rhai_paths());

        // Create a set of valid log param fields (dynamic)
        let valid_dynamic_fields: HashSet<String> = if let Some(abi) = abi {
            abi.events
                .values()
                .flatten()
                .flat_map(|event| &event.inputs)
                .map(|param| format!("log.params.{}", param.name))
                .collect()
        } else {
            HashSet::new()
        };

        // Iterate over accessed variables
        for field in ast_analysis.accessed_variables.iter() {
            let is_static = valid_static_fields.contains(field);
            let is_dynamic = field.starts_with("log.params.");

            if is_static {
                // Static field, always valid
                continue;
            } else if is_dynamic {
                // Dynamic field, check against ABI fields
                if !valid_dynamic_fields.contains(field) {
                    return Err(RhaiScriptValidationError::InvalidAbiField(field.clone()));
                }
            } else {
                // Invalid field
                return Err(RhaiScriptValidationError::InvalidField(field.clone()));
            }
        }

        // Check if return type is boolean
        self.validate_return_type(&ast_analysis)?;

        // Determine if receipt data is required
        let valid_receipt_fields = get_valid_receipt_rhai_paths();
        let requires_receipt = ast_analysis
            .accessed_variables
            .iter()
            .any(|field| valid_receipt_fields.contains(field));

        Ok(RhaiScriptValidationResult { ast_analysis, requires_receipt })
    }

    /// Validates that the script returns a boolean value.
    fn validate_return_type(
        &self,
        ast_analysis: &ScriptAnalysis,
    ) -> Result<(), RhaiScriptValidationError> {
        // Create dummy scope
        let mut scope = rhai::Scope::new();

        // Check if 'tx' object is needed
        if ast_analysis.accessed_variables.iter().any(|v| v.starts_with("tx.")) {
            scope.push("tx", rhai::Map::new());
        }
        // Check if 'log' object is needed
        if ast_analysis.accessed_variables.iter().any(|v| v.starts_with("log.")) {
            let mut log_map = rhai::Map::new();

            // If any 'log.params.*' variable is used, we must create the nested 'params'
            // map.
            if ast_analysis.accessed_variables.iter().any(|v| v.starts_with("log.params.")) {
                // An empty map is sufficient for 'params' because the fields inside it
                // (e.g. 'amount') will resolve to '()' when not found, which is fine
                // for the type checker in most cases
                log_map.insert("params".into(), rhai::Map::new().into());
            }
            scope.push("log", log_map);
        }

        let value = self
            .compiler
            .engine
            .eval_ast_with_scope::<rhai::Dynamic>(&mut scope, &ast_analysis.ast)?;

        if value.is::<bool>() {
            Ok(())
        } else {
            Err(RhaiScriptValidationError::InvalidReturnType(format!(
                "Expected boolean return type, got {}",
                value.type_name()
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RhaiConfig;

    fn create_validator() -> RhaiScriptValidator {
        let config = RhaiConfig::default();
        let compiler = Arc::new(RhaiCompiler::new(config));
        RhaiScriptValidator::new(compiler)
    }

    fn simple_abi_json() -> &'static str {
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

    #[test]
    fn test_validate_return_type_valid() {
        let validator = create_validator();
        let script = "tx.value > 100";
        let analysis = validator.compiler.analyze_script(script).unwrap();
        assert!(validator.validate_return_type(&analysis).is_ok());
    }

    #[test]
    fn test_validate_return_type_invalid() {
        let validator = create_validator();
        let script = "tx.value"; // Not a boolean expression
        let analysis = validator.compiler.analyze_script(script).unwrap();
        let result = validator.validate_return_type(&analysis);
        assert!(result.is_err());
        assert!(matches!(result.err().unwrap(), RhaiScriptValidationError::InvalidReturnType(_)));
    }

    #[test]
    fn test_validate_analysis_valid_tx() {
        let validator = create_validator();
        let script = "tx.value > 100 && tx.from == \"0x123\"";
        let analysis = validator.compiler.analyze_script(script).unwrap();
        let result = validator.validate_analysis(analysis, None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_analysis_valid_static_log() {
        let validator = create_validator();
        let script = "log.name == \"Transfer\" && log.address == \"0xabc\""; // Valid log fields
        let analysis = validator.compiler.analyze_script(script).unwrap();
        let abi = serde_json::from_str(simple_abi_json()).unwrap();
        let result = validator.validate_analysis(analysis, Some(&abi));
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_analysis_valid_dynamic_log() {
        let validator = create_validator();
        let script = "log.params.amount > usdc(10)"; // Valid dynamic log field
        let analysis = validator.compiler.analyze_script(script).unwrap();
        let abi = serde_json::from_str(simple_abi_json()).unwrap();
        let result = validator.validate_analysis(analysis, Some(&abi));
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_analysis_valid_tx_and_log() {
        let validator = create_validator();
        let script = "tx.value > 100 && log.name == \"Transfer\""; // Valid tx and log fields
        let analysis = validator.compiler.analyze_script(script).unwrap();
        let abi = serde_json::from_str(simple_abi_json()).unwrap();
        let result = validator.validate_analysis(analysis, Some(&abi));
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_analysis_valid_static_and_dynamic_log() {
        let validator = create_validator();
        let script = "log.address == \"0xabc\" && log.params.from == \"0x123\"";
        let analysis = validator.compiler.analyze_script(script).unwrap();
        let abi = serde_json::from_str(simple_abi_json()).unwrap();
        let result = validator.validate_analysis(analysis, Some(&abi));
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_analysis_invalid_abi_field() {
        let validator = create_validator();
        let script = "log.params.nonexistent_field == 42"; // Field not in ABI
        let analysis = validator.compiler.analyze_script(script).unwrap();
        let abi = serde_json::from_str(simple_abi_json()).unwrap();
        let result = validator.validate_analysis(analysis, Some(&abi));
        assert!(result.is_err());
        assert!(matches!(result.err().unwrap(), RhaiScriptValidationError::InvalidAbiField(_)));
    }

    #[test]
    fn test_validate_analysis_invalid_static_field() {
        let validator = create_validator();
        let script = "tx.invalid_field == 42"; // Invalid static field
        let analysis = validator.compiler.analyze_script(script).unwrap();
        let result = validator.validate_analysis(analysis, None);
        assert!(result.is_err());
        assert!(matches!(result.err().unwrap(), RhaiScriptValidationError::InvalidField(_)));
    }

    #[test]
    fn test_validate_analysis_invalid_static_field_with_abi() {
        let validator = create_validator();
        let script = "tx.nonexistent_field > 100";
        let abi = serde_json::from_str(simple_abi_json()).unwrap();
        let result = validator.validate_script(script, Some(&abi));

        assert!(result.is_err());
        assert!(matches!(result.err().unwrap(), RhaiScriptValidationError::InvalidField(_)));
    }

    #[test]
    fn test_validate_analysis_sets_requires_receipt() {
        let validator = create_validator();
        let script = "tx.gas_used > gwei(\"21000\")"; // Accessing receipt field
        let analysis = validator.compiler.analyze_script(script).unwrap();
        let result = validator.validate_analysis(analysis, None).unwrap();
        assert!(result.requires_receipt);
    }

    #[test]
    fn test_validate_script_with_syntax_error() {
        let validator = create_validator();
        // Script with an unclosed parenthesis
        let script = "tx.value > (100";
        let result = validator.validate_script(script, None);

        assert!(result.is_err());
        assert!(matches!(result.err().unwrap(), RhaiScriptValidationError::RhaiSyntaxError(_)));
    }

    #[test]
    fn test_validate_empty_script_fails_return_type() {
        let validator = create_validator();
        let script = "";
        let result = validator.validate_script(script, None);

        assert!(result.is_err());
        let err = result.err().unwrap();
        assert!(
            matches!(err, RhaiScriptValidationError::InvalidReturnType(_)),
            "Expected InvalidReturnType, got {:?}",
            err
        );
    }

    #[test]
    fn test_validate_analysis_log_params_access_without_abi() {
        let validator = create_validator();
        let script = "log.params.amount > bigint(100)"; // Accessing log.params without ABI
        let analysis = validator.compiler.analyze_script(script).unwrap();
        let result = validator.validate_analysis(analysis, None);
        assert!(result.is_err());
        assert!(matches!(result.err().unwrap(), RhaiScriptValidationError::InvalidAbiField(_)));
    }

    #[test]
    fn test_validate_literal_boolean_script() {
        let validator = create_validator();
        let script = "true";
        let result = validator.validate_script(script, None).unwrap();

        assert!(result.ast_analysis.accessed_variables.is_empty());
        assert!(!result.requires_receipt);
        assert!(!result.ast_analysis.accesses_log_variable);
    }
}
