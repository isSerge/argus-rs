use std::collections::HashSet;

use alloy::json_abi::JsonAbi;
use thiserror::Error;

use super::{
    RhaiCompiler, RhaiCompilerError, ScriptAnalysis, get_valid_log_rhai_paths,
    get_valid_receipt_rhai_paths, get_valid_tx_rhai_paths,
};

/// Validates Rhai scripts against allowed fields and ABI.
pub struct RhaiScriptValidator {
    compiler: RhaiCompiler,
}

/// Errors that can occur during Rhai script validation.
#[derive(Debug, Error)]
pub enum RhaiScriptValidationError {
    /// The script contains an invalid field access.
    #[error("Invalid field access: {0}")]
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
pub struct RhaiScriptValidationResult {
    pub ast_analysis: ScriptAnalysis,
    pub requires_receipt: bool,
}

impl RhaiScriptValidator {
    /// Creates a new Rhai script validator with the given compiler.
    pub fn new(compiler: RhaiCompiler) -> Self {
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
                .flat_map(|event| event.iter().map(|f| format!("log.params.{}", f.name)))
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

        // Validate log fields against ABI if provided
        if ast_analysis.accesses_log_variable && abi.is_none() {
            return Err(RhaiScriptValidationError::MissingAbi);
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

        // We don't need real data, just placeholders for the engine to resolve types.
        // A simple map is enough for fields like `tx.value` or `log.address`.
        if ast_analysis.accessed_variables.iter().any(|v| v.starts_with("tx.")) {
            scope.push("tx", rhai::Map::new());
        }
        if ast_analysis.accessed_variables.iter().any(|v| v.starts_with("log.")) {
            scope.push("log", rhai::Map::new());
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
