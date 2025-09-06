//! A utility module for traversing a Rhai AST to extract information.
//! This module is the core of static analysis for Rhai scripts.

use std::collections::HashSet;

use rhai::{AST, Expr, Stmt};

/// The result of a script analysis.
#[derive(Debug, Default)]
pub struct ScriptAnalysisResult {
    /// A set of all unique, fully-qualified variable paths accessed in the
    /// script.
    pub accessed_variables: HashSet<String>,

    /// True if the script accesses the `log` variable.
    pub accesses_log_variable: bool,

    /// A set of local variables defined within the script using `let`.
    pub local_variables: HashSet<String>,
}

/// Traverses a compiled `AST` and returns a `ScriptAnalysisResult` containing
/// accessed variables and other metadata.
///
/// This is the primary entry point for the analyzer.
pub fn analyze_ast(ast: &AST) -> ScriptAnalysisResult {
    let mut result = ScriptAnalysisResult::default();
    for stmt in ast.statements() {
        walk_stmt(stmt, &mut result);
    }
    result
}

/// Recursively walks a statement (`Stmt`) to find expressions.
fn walk_stmt(stmt: &Stmt, result: &mut ScriptAnalysisResult) {
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
            // The `for_loop.0` and `for_loop.1` are variable names (e.g., `item` in `for
            // item in ...`)
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
            // Add the defined variable name to our local_variables set.
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
        Stmt::Return(Some(expr), _, _) | Stmt::BreakLoop(Some(expr), _, _) =>
            walk_expr(expr, result),
        Stmt::Import(import_data, _) => {
            walk_expr(&import_data.0, result);
        }
        Stmt::Noop(_)
        | Stmt::Return(None, _, _)
        | Stmt::BreakLoop(None, _, _)
        | Stmt::Export(_, _)
        | Stmt::Share(_) => {}

        _ => {
            // For all other statements, we do not track them as variable paths.
            // This includes comments, empty statements, etc.
            // The main walker will handle these cases by recursing into their
            // components.
        }
    }
}

/// Recursively walks an expression (`Expr`) to find and record variable access
/// paths.
fn walk_expr(expr: &Expr, result: &mut ScriptAnalysisResult) {
    if let Some(path) = get_full_variable_path(expr) {
        if !result.accesses_log_variable && path.starts_with("log") {
            result.accesses_log_variable = true;
        }
        result.accessed_variables.insert(path);
        // For Index, also collect index variable if present
        if let Expr::Index(binary_expr, _, _) = expr
            && let Some(index_path) = get_full_variable_path(&binary_expr.rhs)
        {
            if !result.accesses_log_variable && index_path.starts_with("log") {
                result.accesses_log_variable = true;
            }
            result.accessed_variables.insert(index_path);
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
            // For index, collect index variable if present
            if let Some(index_path) = get_full_variable_path(&binary_expr.rhs) {
                if !result.accesses_log_variable && index_path.starts_with("log") {
                    result.accesses_log_variable = true;
                }
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
        Expr::Variable(_, _, _) | Expr::Property(_, _) => {}
        Expr::DynamicConstant(_, _)
        | Expr::BoolConstant(_, _)
        | Expr::IntegerConstant(_, _)
        | Expr::CharConstant(_, _)
        | Expr::StringConstant(_, _)
        | Expr::Unit(_)
        | Expr::ThisPtr(_)
        | Expr::FloatConstant(_, _) => {}
        _ => {}
    }
}

/// Attempts to reconstruct a full variable path (e.g., "tx.value") from an
/// expression.
fn get_full_variable_path(expr: &Expr) -> Option<String> {
    // Recursively collect property/index chains in left-to-right order
    fn collect_path(expr: &Expr, parts: &mut Vec<String>) -> bool {
        match expr {
            Expr::Dot(binary_expr, _, _) => {
                let mut ok = collect_path(&binary_expr.lhs, parts);
                ok &= collect_path(&binary_expr.rhs, parts);
                ok
            }
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
        assert!(!result.accesses_log_variable);
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
        assert!(result.accesses_log_variable);
    }

    #[test]
    fn test_multiple_variables_and_coalesce() {
        let result = analyze_script("tx.from ?? fallback_addr.address").unwrap();
        assert_eq!(
            result.accessed_variables,
            HashSet::from(["tx.from".to_string(), "fallback_addr.address".to_string(),])
        );
        assert!(!result.accesses_log_variable);
    }

    #[test]
    fn test_deeply_nested_variable() {
        let script = r#"log.params.level_one.level_two.user == "admin""#;
        let result = analyze_script(script).unwrap();
        assert_eq!(
            result.accessed_variables,
            HashSet::from(["log.params.level_one.level_two.user".to_string()])
        );
        assert!(result.accesses_log_variable);
    }

    #[test]
    fn test_variables_in_function_calls() {
        let result = analyze_script("my_func(tx.value, log.params.user, 42)").unwrap();
        assert_eq!(
            result.accessed_variables,
            HashSet::from(["tx.value".to_string(), "log.params.user".to_string()])
        );
        assert!(result.accesses_log_variable);
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
        assert!(!result.accesses_log_variable);
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
        assert!(!result.accesses_log_variable);
    }

    #[test]
    fn test_variables_in_string_or_comments_are_ignored() {
        let script = r#"
            // This is a comment about tx.value
            let x = "this string mentions log.name";
            tx.from == "0x123"
        "#;
        let result = analyze_script(script).unwrap();
        assert_eq!(result.accessed_variables, HashSet::from(["tx.from".to_string()]));
        assert!(!result.accesses_log_variable);
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
        assert!(!result.accesses_log_variable);
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
        assert!(!result.accesses_log_variable);
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
        assert!(result.accesses_log_variable);
    }

    #[test]
    fn test_no_variables() {
        let script = "1 + 1 == 2";
        let result = analyze_script(script).unwrap();
        assert!(result.accessed_variables.is_empty());
        assert!(!result.accesses_log_variable);
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
        assert!(result.accesses_log_variable);
    }

    #[test]
    fn test_log_variable_only() {
        let script = "log.name == \"Transfer\"";
        let result = analyze_script(script).unwrap();
        assert_eq!(result.accessed_variables, HashSet::from(["log.name".to_string()]));
        assert!(result.accesses_log_variable);
    }
}
