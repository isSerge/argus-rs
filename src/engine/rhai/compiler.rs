//! Rhai compiler and analysis module.
//! Compiles Rhai scripts into an intermediate representation (AST), performs
//! analysis and stores them in local cache.

use std::{collections::HashSet, sync::Arc};

use dashmap::DashMap;
use rhai::{AST, Engine};
use sha2::{Digest, Sha256};
use thiserror::Error;

use super::{ast_analysis, create_engine};
use crate::config::RhaiConfig;

/// Represents the result of analyzing a Rhai script.
#[derive(Clone, Debug)]
pub struct ScriptAnalysis {
    /// The compiled AST of the Rhai script.
    pub ast: Arc<AST>,

    /// A set of variable paths accessed in the script.
    /// This is used to determine which variables are read or written to.
    pub accessed_variables: Arc<HashSet<String>>,

    /// True if the script accesses the `log` variable.
    pub accesses_log_variable: bool,

    /// A set of local variables defined within the script.
    pub local_variables: HashSet<String>,
}

/// A type alias for the hash of a Rhai script.
type ScriptHash = [u8; 32];

/// The Rhai compiler that compiles scripts and analyzes their ASTs.
/// It caches the results to avoid redundant compilations.
#[derive(Debug)]
pub struct RhaiCompiler {
    /// The Rhai engine used for compiling scripts.
    pub engine: Arc<Engine>,
    /// A cache that stores the analysis results of scripts.
    cache: DashMap<ScriptHash, ScriptAnalysis>,
}

/// Errors that can occur during Rhai compilation or analysis.
#[derive(Debug, Clone, Error)]
pub enum RhaiCompilerError {
    /// Error that occurs during script compilation.
    #[error("Rhai compilation error: {0}")]
    CompilationError(#[from] rhai::ParseError),
}

impl RhaiCompiler {
    /// Creates a new instance of the Rhai compiler.
    pub fn new(rhai_config: RhaiConfig) -> Self {
        let engine = create_engine(rhai_config);

        RhaiCompiler { engine: Arc::new(engine), cache: DashMap::new() }
    }

    /// A helper function to compute the hash of a script.
    fn hash_script(script: &str) -> ScriptHash {
        let mut hasher = Sha256::new();
        hasher.update(script.as_bytes());
        hasher.finalize().into()
    }

    /// Analyzes a Rhai script, compiling it into an AST and extracting accessed
    /// variables. If the script has been analyzed before, it retrieves the
    /// result from the cache.
    pub fn analyze_script(&self, script: &str) -> Result<ScriptAnalysis, RhaiCompilerError> {
        // Compute the hash of the script to use as a cache key.
        let key = Self::hash_script(script);

        // Check the cache first
        if let Some(cached) = self.cache.get(&key) {
            return Ok(cached.value().clone());
        }

        // Compile the script
        let ast = self.engine.compile(script)?;
        // Analyze the AST to extract accessed variables
        let analysis_result = ast_analysis::analyze_ast(&ast);

        // Create the ScriptAnalysis struct
        let analysis = ScriptAnalysis {
            ast: Arc::new(ast),
            accessed_variables: Arc::new(analysis_result.accessed_variables),
            accesses_log_variable: analysis_result.accesses_log_variable,
            local_variables: analysis_result.local_variables,
        };

        // Store the analysis in the cache
        self.cache.insert(key, analysis.clone());

        // Return the analysis result
        Ok(analysis)
    }

    /// Convenience method to get the AST of a script without needing to handle
    /// the analysis result.
    pub fn get_ast(&self, script: &str) -> Result<Arc<AST>, RhaiCompilerError> {
        self.analyze_script(script).map(|analysis| analysis.ast)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_analyze_valid_script_tx_only() {
        let config = RhaiConfig::default();
        let compiler = RhaiCompiler::new(config);
        let script = "tx.value > 100";

        let result = compiler.analyze_script(script);
        assert!(result.is_ok());

        let analysis = result.unwrap();

        // Check that the accessed variables are correct.
        let expected_vars: HashSet<String> = HashSet::from(["tx.value".to_string()]);
        assert_eq!(*analysis.accessed_variables, expected_vars);
        assert!(!analysis.accesses_log_variable);

        // Check that the AST is present.
        assert!(Arc::strong_count(&analysis.ast) >= 1);
    }

    #[test]
    fn test_analyze_valid_script_with_log() {
        let config = RhaiConfig::default();
        let compiler = RhaiCompiler::new(config);
        let script = "tx.value > 100 && log.name == \"Transfer\"";

        let result = compiler.analyze_script(script);
        assert!(result.is_ok());

        let analysis = result.unwrap();

        // Check that the accessed variables are correct.
        let expected_vars: HashSet<String> =
            HashSet::from(["tx.value".to_string(), "log.name".to_string()]);
        assert_eq!(*analysis.accessed_variables, expected_vars);
        assert!(analysis.accesses_log_variable);
    }

    #[test]
    fn test_analyze_invalid_script() {
        let config = RhaiConfig::default();
        let compiler = RhaiCompiler::new(config);
        // This script is invalid because of the unclosed quote.
        let script = "tx.value > 100 && tx.from == \"0x123";

        let result = compiler.analyze_script(script);
        assert!(result.is_err());

        assert!(matches!(result.err().unwrap(), RhaiCompilerError::CompilationError(_)));
    }

    #[test]
    fn test_caching_works() {
        let config = RhaiConfig::default();
        let compiler = RhaiCompiler::new(config);
        let script = "let x = 42; x > 0";

        // 1. First call: This will compile, analyze, and cache.
        let first_result = compiler.analyze_script(script).unwrap();

        // Check the cache. It should now have one entry.
        assert_eq!(compiler.cache.len(), 1);

        // 2. Second call: This should hit the cache.
        let second_result = compiler.analyze_script(script).unwrap();

        // The cache should still have only one entry.
        assert_eq!(compiler.cache.len(), 1);

        // 3. Verify that the returned objects are the same shared instance.
        // We use `Arc::ptr_eq` to check if they point to the exact same memory
        // location.
        assert!(Arc::ptr_eq(&first_result.ast, &second_result.ast));
        assert!(Arc::ptr_eq(&first_result.accessed_variables, &second_result.accessed_variables));
        assert_eq!(first_result.accesses_log_variable, second_result.accesses_log_variable);
    }

    #[test]
    fn test_get_ast_convenience_method() {
        let config = RhaiConfig::default();
        let compiler = RhaiCompiler::new(config);
        let script = "1 + 1 == 2";

        let result = compiler.get_ast(script);
        assert!(result.is_ok());

        // The AST should be cached after the first call.
        assert_eq!(compiler.cache.len(), 1);
        let key = RhaiCompiler::hash_script(script);
        let cached_entry = compiler.cache.get(&key).unwrap();
        assert!(Arc::ptr_eq(&result.unwrap(), &cached_entry.ast));
    }

    #[test]
    fn test_caching_with_different_scripts() {
        let config = RhaiConfig::default();
        let compiler = RhaiCompiler::new(config);
        let script1 = "tx.value > 100";
        let script2 = "log.name == \"Transfer\"";

        // Analyze both scripts.
        let analysis1 = compiler.analyze_script(script1).unwrap();
        let analysis2 = compiler.analyze_script(script2).unwrap();

        let key1 = RhaiCompiler::hash_script(script1);
        let key2 = RhaiCompiler::hash_script(script2);

        // The cache should now contain two distinct entries.
        assert_eq!(compiler.cache.len(), 2);
        assert!(compiler.cache.contains_key(&key1));
        assert!(compiler.cache.contains_key(&key2));

        assert!(!analysis1.accesses_log_variable);
        assert!(analysis2.accesses_log_variable);
    }

    #[test]
    fn test_empty_script_is_valid() {
        let config = RhaiConfig::default();
        let compiler = RhaiCompiler::new(config);
        let script = ""; // An empty script

        let result = compiler.analyze_script(script);
        assert!(result.is_ok());

        let analysis = result.unwrap();
        // An empty script should access no variables.
        assert!(analysis.accessed_variables.is_empty());
        assert!(!analysis.accesses_log_variable);
    }

    #[test]
    fn test_script_with_comments_only() {
        let config = RhaiConfig::default();
        let compiler = RhaiCompiler::new(config);
        let script = r#"
            // This is a test script.
            // It only contains comments.
        "#;

        let result = compiler.analyze_script(script);
        assert!(result.is_ok());

        let analysis = result.unwrap();
        // A script with only comments should access no variables.
        assert!(analysis.accessed_variables.is_empty());
        assert!(!analysis.accesses_log_variable);
    }

    #[test]
    fn test_script_with_local_variable() {
        let config = RhaiConfig::default();
        let compiler = RhaiCompiler::new(config);
        let script = r#"
            let x = 10;
            x > 5
        "#;

        let result = compiler.analyze_script(script);
        assert!(result.is_ok());

        let analysis = result.unwrap();
        // The accessed variables should include only 'x'.
        let expected_vars: HashSet<String> = HashSet::from(["x".to_string()]);
        assert_eq!(*analysis.accessed_variables, expected_vars);
        assert!(!analysis.accesses_log_variable);

        // The local variables should include 'x'.
        let expected_locals: HashSet<String> = HashSet::from(["x".to_string()]);
        assert_eq!(analysis.local_variables, expected_locals);
    }
}
