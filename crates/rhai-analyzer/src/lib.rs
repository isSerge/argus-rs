//! Static analysis utilities for Rhai scripts.
//!
//! Traverses a compiled [`rhai::AST`] to extract variable access paths,
//! local variable definitions, and string literal comparisons. The output is
//! entirely domain-agnostic — callers decide what to do with the results.

use std::collections::{HashMap, HashSet};

use rhai::{AST, Expr, Stmt};

/// The result of a static analysis pass over a Rhai [`AST`].
#[derive(Debug, Default)]
pub struct ScriptAnalysisResult {
    /// All unique, fully-qualified variable paths accessed in the script
    /// (e.g. `"tx.value"`, `"log.name"`).
    pub accessed_variables: HashSet<String>,

    /// Local variables defined within the script via `let` or loop iteration
    /// variables.
    pub local_variables: HashSet<String>,

    /// Maps a fully-qualified variable path to the set of string literals it
    /// is compared against (via `==` or `!=`) anywhere in the script.
    ///
    /// Example: `log.name == "Transfer"` produces
    /// `"log.name" => {"Transfer"}`.
    pub string_comparisons: HashMap<String, HashSet<String>>,
}

/// Traverses a compiled [`AST`] and returns a [`ScriptAnalysisResult`].
///
/// This is the primary entry point for the analyzer.
pub fn analyze_ast(ast: &AST) -> ScriptAnalysisResult {
    let mut result = ScriptAnalysisResult::default();
    for stmt in ast.statements() {
        walk_stmt(stmt, &mut result);
    }
    result
}

// ---------------------------------------------------------------------------
// Statement walker
// ---------------------------------------------------------------------------

fn walk_stmt(stmt: &Stmt, result: &mut ScriptAnalysisResult) {
    // Check for string comparisons at the statement level before the main
    // structural dispatch below.
    match stmt {
        Stmt::Expr(expr) => check_for_string_comparisons(expr, result),
        Stmt::FnCall(fn_call_expr, _) => {
            let expr = Expr::FnCall(fn_call_expr.clone(), rhai::Position::NONE);
            check_for_string_comparisons(&expr, result);
        }
        _ => {}
    }

    match stmt {
        Stmt::Expr(expr) => walk_expr(expr, result),
        Stmt::Block(stmt_block) =>
            for s in stmt_block.statements() {
                walk_stmt(s, result);
            },
        Stmt::If(flow_control, _) => {
            walk_expr(&flow_control.expr, result);
            for s in flow_control.body.statements() {
                walk_stmt(s, result);
            }
            for s in flow_control.branch.statements() {
                walk_stmt(s, result);
            }
        }
        Stmt::While(flow_control, _) => {
            walk_expr(&flow_control.expr, result);
            for s in flow_control.body.statements() {
                walk_stmt(s, result);
            }
        }
        Stmt::Do(flow_control, _, _) => {
            for s in flow_control.body.statements() {
                walk_stmt(s, result);
            }
            walk_expr(&flow_control.expr, result);
        }
        Stmt::For(for_loop, _) => {
            result.local_variables.insert(for_loop.0.name.to_string());
            if let Some(second_var) = &for_loop.1 {
                result.local_variables.insert(second_var.name.to_string());
            }
            walk_expr(&for_loop.2.expr, result);
            for s in for_loop.2.body.statements() {
                walk_stmt(s, result);
            }
        }
        Stmt::Var(var_definition, _, _) => {
            result.local_variables.insert(var_definition.0.name.to_string());
            walk_expr(&var_definition.1, result);
        }
        Stmt::Assignment(assignment) => {
            walk_expr(&assignment.1.lhs, result);
            walk_expr(&assignment.1.rhs, result);
        }
        Stmt::FnCall(fn_call_expr, _) =>
            for arg in &fn_call_expr.args {
                walk_expr(arg, result);
            },
        Stmt::Switch(switch_data, _) => {
            let (expr, cases_collection) = &**switch_data;
            walk_expr(expr, result);
            for case_expr in &cases_collection.expressions {
                walk_expr(&case_expr.lhs, result);
                walk_expr(&case_expr.rhs, result);
            }
        }
        Stmt::TryCatch(flow_control, _) => {
            for s in flow_control.body.statements() {
                walk_stmt(s, result);
            }
            for s in flow_control.branch.statements() {
                walk_stmt(s, result);
            }
        }
        Stmt::Return(Some(expr), _, _) | Stmt::BreakLoop(Some(expr), _, _) => {
            walk_expr(expr, result);
        }
        Stmt::Import(import_data, _) => {
            walk_expr(&import_data.0, result);
        }
        Stmt::Noop(_)
        | Stmt::Return(None, _, _)
        | Stmt::BreakLoop(None, _, _)
        | Stmt::Export(_, _)
        | Stmt::Share(_) => {}
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Expression walker
// ---------------------------------------------------------------------------

fn walk_expr(expr: &Expr, result: &mut ScriptAnalysisResult) {
    check_for_string_comparisons(expr, result);

    if let Some(path) = get_full_variable_path(expr) {
        result.accessed_variables.insert(path);
        if let Expr::Index(binary_expr, _, _) = expr {
            if let Some(index_path) = get_full_variable_path(&binary_expr.rhs) {
                result.accessed_variables.insert(index_path);
            }
        }
        return;
    }

    match expr {
        Expr::Dot(binary_expr, _, _) => {
            walk_expr(&binary_expr.lhs, result);
            walk_expr(&binary_expr.rhs, result);
        }
        Expr::Index(binary_expr, _, _) => {
            walk_expr(&binary_expr.lhs, result);
            if let Some(index_path) = get_full_variable_path(&binary_expr.rhs) {
                result.accessed_variables.insert(index_path);
            } else {
                walk_expr(&binary_expr.rhs, result);
            }
        }
        Expr::MethodCall(method_call_expr, _) =>
            for arg in &method_call_expr.args {
                walk_expr(arg, result);
            },
        Expr::FnCall(fn_call_expr, _) =>
            for arg in &fn_call_expr.args {
                walk_expr(arg, result);
            },
        Expr::And(expr_vec, _) | Expr::Or(expr_vec, _) | Expr::Coalesce(expr_vec, _) => {
            for e in &**expr_vec {
                walk_expr(e, result);
            }
        }
        Expr::Array(expr_vec, _) | Expr::InterpolatedString(expr_vec, _) =>
            for e in expr_vec {
                walk_expr(e, result);
            },
        Expr::Map(map_data, _) =>
            for (_, value_expr) in &map_data.0 {
                walk_expr(value_expr, result);
            },
        Expr::Stmt(stmt_block) =>
            for s in stmt_block.statements() {
                walk_stmt(s, result);
            },
        Expr::Custom(custom_expr, _) =>
            for e in &custom_expr.inputs {
                walk_expr(e, result);
            },
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Path reconstruction
// ---------------------------------------------------------------------------

/// Attempts to reconstruct a full dotted variable path (e.g. `"tx.value"`)
/// from an expression.
fn get_full_variable_path(expr: &Expr) -> Option<String> {
    fn collect_path(expr: &Expr, parts: &mut Vec<String>) -> bool {
        match expr {
            Expr::Dot(binary_expr, _, _) =>
                collect_path(&binary_expr.lhs, parts) && collect_path(&binary_expr.rhs, parts),
            Expr::Property(prop_info, _) => {
                parts.push(prop_info.2.to_string());
                true
            }
            Expr::Variable(var_info, _, _) => {
                parts.push(var_info.1.to_string());
                true
            }
            Expr::Index(binary_expr, _, _) => collect_path(&binary_expr.lhs, parts),
            _ => false,
        }
    }

    let mut path_parts = Vec::new();
    if collect_path(expr, &mut path_parts) && !path_parts.is_empty() {
        Some(path_parts.join("."))
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// String comparison tracking
// ---------------------------------------------------------------------------

/// Recursively checks an expression for `variable_path == "literal"` (or
/// `!=`) patterns and records them in
/// [`ScriptAnalysisResult::string_comparisons`].
fn check_for_string_comparisons(expr: &Expr, result: &mut ScriptAnalysisResult) {
    match expr {
        Expr::FnCall(fn_call_expr, _) => {
            if fn_call_expr.namespace.is_empty() && fn_call_expr.args.len() == 2 {
                match fn_call_expr.name.as_str() {
                    "==" | "!=" => {
                        record_string_comparison(
                            &fn_call_expr.args[0],
                            &fn_call_expr.args[1],
                            result,
                        );
                        record_string_comparison(
                            &fn_call_expr.args[1],
                            &fn_call_expr.args[0],
                            result,
                        );
                    }
                    _ =>
                        for arg in &fn_call_expr.args {
                            check_for_string_comparisons(arg, result);
                        },
                }
            } else {
                for arg in &fn_call_expr.args {
                    check_for_string_comparisons(arg, result);
                }
            }
        }
        Expr::And(expr_vec, _) | Expr::Or(expr_vec, _) =>
            for e in &**expr_vec {
                check_for_string_comparisons(e, result);
            },
        Expr::Dot(binary_expr, _, _) | Expr::Index(binary_expr, _, _) => {
            check_for_string_comparisons(&binary_expr.lhs, result);
            check_for_string_comparisons(&binary_expr.rhs, result);
        }
        Expr::MethodCall(method_call_expr, _) =>
            for arg in &method_call_expr.args {
                check_for_string_comparisons(arg, result);
            },
        Expr::Array(expr_vec, _) =>
            for e in expr_vec {
                check_for_string_comparisons(e, result);
            },
        Expr::Stmt(stmt_block) =>
            for s in stmt_block.statements() {
                if let Stmt::Expr(inner_expr) = s {
                    check_for_string_comparisons(inner_expr, result);
                }
            },
        _ => {}
    }
}

/// If `lhs` is a variable path and `rhs` is a string literal, records the
/// comparison in `result.string_comparisons`.
fn record_string_comparison(lhs: &Expr, rhs: &Expr, result: &mut ScriptAnalysisResult) {
    if let Some(var_path) = get_full_variable_path(lhs) {
        if let Expr::StringConstant(string_val, _) = rhs {
            result.string_comparisons.entry(var_path).or_default().insert(string_val.to_string());
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use rhai::{Engine, ParseError};

    use super::*;

    fn analyze_script(script: &str) -> Result<ScriptAnalysisResult, ParseError> {
        let engine = Engine::new();
        let ast = engine.compile(script)?;
        Ok(analyze_ast(&ast))
    }

    #[test]
    fn test_simple_binary_op() {
        let result = analyze_script("tx.value > 100").unwrap();
        assert_eq!(result.accessed_variables, HashSet::from(["tx.value".to_string()]));
    }

    #[test]
    fn test_logical_operators() {
        let script = r#"tx.from == owner && log.name != "Transfer" || block.number > 1000"#;
        let result = analyze_script(script).unwrap();
        assert_eq!(
            result.accessed_variables,
            HashSet::from([
                "tx.from".to_string(),
                "owner".to_string(),
                "log.name".to_string(),
                "block.number".to_string(),
            ])
        );
    }

    #[test]
    fn test_multiple_variables_and_coalesce() {
        let result = analyze_script("tx.from ?? fallback_addr.address").unwrap();
        assert_eq!(
            result.accessed_variables,
            HashSet::from(["tx.from".to_string(), "fallback_addr.address".to_string()])
        );
    }

    #[test]
    fn test_deeply_nested_variable() {
        let script = r#"log.params.level_one.level_two.user == "admin""#;
        let result = analyze_script(script).unwrap();
        assert_eq!(
            result.accessed_variables,
            HashSet::from(["log.params.level_one.level_two.user".to_string()])
        );
    }

    #[test]
    fn test_variables_in_function_calls() {
        let result = analyze_script("my_func(tx.value, log.params.user, 42)").unwrap();
        assert_eq!(
            result.accessed_variables,
            HashSet::from(["tx.value".to_string(), "log.params.user".to_string()])
        );
    }

    #[test]
    fn test_variables_in_let_and_if() {
        let script = r#"
            let threshold = config.min_value;
            if tx.value > threshold && tx.to != blacklist.address {
                true
            } else {
                false
            }
        "#;
        let result = analyze_script(script).unwrap();
        assert_eq!(
            result.accessed_variables,
            HashSet::from([
                "config.min_value".to_string(),
                "tx.value".to_string(),
                "threshold".to_string(),
                "tx.to".to_string(),
                "blacklist.address".to_string()
            ])
        );
    }

    #[test]
    fn test_variables_in_loops() {
        let script = r#"
            for item in tx.items {
                if item.cost > max_cost {
                    return false;
                }
            }
            while x < limit {
                x = x + 1;
            }
        "#;
        let result = analyze_script(script).unwrap();
        assert_eq!(
            result.accessed_variables,
            HashSet::from([
                "tx.items".to_string(),
                "item.cost".to_string(),
                "max_cost".to_string(),
                "x".to_string(),
                "limit".to_string(),
            ])
        );
    }

    #[test]
    fn test_variables_in_strings_or_comments_are_ignored() {
        let script = r#"
            // This is a comment about tx.value
            let x = "this string mentions log.name";
            tx.from == "0x123"
        "#;
        let result = analyze_script(script).unwrap();
        assert_eq!(result.accessed_variables, HashSet::from(["tx.from".to_string()]));
    }

    #[test]
    fn test_indexing_expression() {
        let script = r#"tx.logs[0].name == "Transfer" && some_array[tx.index] > 100"#;
        let result = analyze_script(script).unwrap();
        assert_eq!(
            result.accessed_variables,
            HashSet::from([
                "tx.logs".to_string(),
                "some_array".to_string(),
                "tx.index".to_string(),
            ])
        );
    }

    #[test]
    fn test_method_calls() {
        let script = r#"my_array.contains(tx.value) && other_var.to_string() == "hello""#;
        let result = analyze_script(script).unwrap();
        assert_eq!(
            result.accessed_variables,
            HashSet::from([
                "my_array".to_string(),
                "tx.value".to_string(),
                "other_var".to_string(),
            ])
        );
    }

    #[test]
    fn test_switch_statement() {
        let script = r#"
            switch tx.action {
                "transfer" => do_transfer(log.params.amount),
                "approve" if log.approved => do_approve(),
                _ => do_nothing(contract.address)
            }
        "#;
        let result = analyze_script(script).unwrap();
        assert_eq!(
            result.accessed_variables,
            HashSet::from([
                "tx.action".to_string(),
                "log.params.amount".to_string(),
                "log.approved".to_string(),
                "contract.address".to_string(),
            ])
        );
    }

    #[test]
    fn test_no_variables() {
        let result = analyze_script("1 + 1 == 2").unwrap();
        assert!(result.accessed_variables.is_empty());
    }

    #[test]
    fn test_array_and_map_literals() {
        let script = r#"
            let my_array = [tx.value, log.topic];
            let my_map = #{ a: some.value, b: 42 };
            my_array[0] > my_map.a
        "#;
        let result = analyze_script(script).unwrap();
        assert_eq!(
            result.accessed_variables,
            HashSet::from([
                "tx.value".to_string(),
                "log.topic".to_string(),
                "some.value".to_string(),
                "my_array".to_string(),
                "my_map.a".to_string(),
            ])
        );
    }

    #[test]
    fn test_string_comparison_simple() {
        let result = analyze_script(r#"log.name == "Transfer""#).unwrap();
        assert_eq!(result.accessed_variables, HashSet::from(["log.name".to_string()]));
        let names = result.string_comparisons.get("log.name").unwrap();
        assert_eq!(names, &HashSet::from(["Transfer".to_string()]));
    }

    #[test]
    fn test_string_comparison_reversed() {
        let result = analyze_script(r#""Approval" == log.name"#).unwrap();
        let names = result.string_comparisons.get("log.name").unwrap();
        assert_eq!(names, &HashSet::from(["Approval".to_string()]));
    }

    #[test]
    fn test_string_comparison_in_logical_or() {
        let result = analyze_script(r#"tx.value > 100 || log.name == "Deposit""#).unwrap();
        let names = result.string_comparisons.get("log.name").unwrap();
        assert_eq!(names, &HashSet::from(["Deposit".to_string()]));
    }

    #[test]
    fn test_string_comparison_multiple_values() {
        let result = analyze_script(r#"log.name == "Transfer" || log.name == "Approval""#).unwrap();
        let names = result.string_comparisons.get("log.name").unwrap();
        assert_eq!(names, &HashSet::from(["Transfer".to_string(), "Approval".to_string()]));
    }

    #[test]
    fn test_string_comparison_inequality() {
        let result = analyze_script(r#"log.name != "Transfer""#).unwrap();
        let names = result.string_comparisons.get("log.name").unwrap();
        assert_eq!(names, &HashSet::from(["Transfer".to_string()]));
    }

    #[test]
    fn test_string_comparison_different_path() {
        let result = analyze_script(r#"tx.status == "success" && tx.type != "mint""#).unwrap();
        let statuses = result.string_comparisons.get("tx.status").unwrap();
        assert_eq!(statuses, &HashSet::from(["success".to_string()]));
        let types = result.string_comparisons.get("tx.type").unwrap();
        assert_eq!(types, &HashSet::from(["mint".to_string()]));
    }
}
