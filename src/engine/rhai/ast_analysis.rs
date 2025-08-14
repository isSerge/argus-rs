//! A utility module for traversing a Rhai AST to extract information.
//! This module is the core of static analysis for Rhai scripts.

use rhai::{AST, Expr, Stmt};
use std::collections::HashSet;

/// Traverses a compiled `AST` and returns a set of all unique, fully-qualified
/// variable paths accessed in the script.
///
/// This is the primary entry point for the analyzer.
pub fn get_accessed_variables(ast: &AST) -> HashSet<String> {
    let mut variables = HashSet::new();
    for stmt in ast.statements() {
        walk_stmt(stmt, &mut variables);
    }
    variables
}

/// Recursively walks a statement (`Stmt`) to find expressions.
fn walk_stmt(stmt: &Stmt, variables: &mut HashSet<String>) {
    match stmt {
        Stmt::Expr(expr) => walk_expr(expr, variables),
        Stmt::Block(stmt_block) => {
            for s in stmt_block.statements() {
                walk_stmt(s, variables);
            }
        }
        Stmt::If(flow_control, _) => {
            walk_expr(&flow_control.expr, variables);
            for s in flow_control.body.statements() {
                walk_stmt(s, variables);
            }
            for s in flow_control.branch.statements() {
                walk_stmt(s, variables);
            }
        }
        Stmt::While(flow_control, _) => {
            walk_expr(&flow_control.expr, variables);
            for s in flow_control.body.statements() {
                walk_stmt(s, variables);
            }
        }
        Stmt::Do(flow_control, _, _) => {
            for s in flow_control.body.statements() {
                walk_stmt(s, variables);
            }
            walk_expr(&flow_control.expr, variables);
        }
        Stmt::For(for_loop, _) => {
            walk_expr(&for_loop.2.expr, variables);
            for s in for_loop.2.body.statements() {
                walk_stmt(s, variables);
            }
        }
        Stmt::Var(var_definition, _, _) => {
            walk_expr(&var_definition.1, variables);
        }
        Stmt::Assignment(assignment) => {
            walk_expr(&assignment.1.lhs, variables);
            walk_expr(&assignment.1.rhs, variables);
        }
        Stmt::FnCall(fn_call_expr, _) => {
            for arg in &fn_call_expr.args {
                walk_expr(arg, variables);
            }
        }
        Stmt::Switch(switch_data, _) => {
            let (expr, cases_collection) = &**switch_data;
            walk_expr(expr, variables);
            for case_expr in &cases_collection.expressions {
                walk_expr(&case_expr.lhs, variables);
                walk_expr(&case_expr.rhs, variables);
            }
        }
        Stmt::TryCatch(flow_control, _) => {
            for s in flow_control.body.statements() {
                walk_stmt(s, variables);
            }
            for s in flow_control.branch.statements() {
                walk_stmt(s, variables);
            }
        }
        Stmt::Return(Some(expr), _, _) | Stmt::BreakLoop(Some(expr), _, _) => {
            walk_expr(expr, variables)
        }
        Stmt::Import(import_data, _) => {
            walk_expr(&import_data.0, variables);
        }
        Stmt::Noop(_)
        | Stmt::Return(None, _, _)
        | Stmt::BreakLoop(None, _, _)
        | Stmt::Export(_, _)
        | Stmt::Share(_) => {}

        _ => {
            // For all other statements, we do not track them as variable paths.
            // This includes comments, empty statements, etc.
            // The main walker will handle these cases by recursing into their components.
        }
    }
}

/// Recursively walks an expression (`Expr`) to find and record variable access paths.
fn walk_expr(expr: &Expr, variables: &mut HashSet<String>) {
    if let Some(path) = get_full_variable_path(expr) {
        variables.insert(path);
        // For Index, also collect index variable if present
        if let Expr::Index(binary_expr, _, _) = expr
            && let Some(index_path) = get_full_variable_path(&binary_expr.rhs) {
                variables.insert(index_path);
            }
        return;
    }

    match expr {
        Expr::Dot(binary_expr, _, _) => {
            walk_expr(&binary_expr.lhs, variables);
            walk_expr(&binary_expr.rhs, variables);
        }
        Expr::Index(binary_expr, _, _) => {
            walk_expr(&binary_expr.lhs, variables);
            // For index, collect index variable if present
            if let Some(index_path) = get_full_variable_path(&binary_expr.rhs) {
                variables.insert(index_path);
            } else {
                walk_expr(&binary_expr.rhs, variables);
            }
        }
        Expr::MethodCall(method_call_expr, _) => {
            for arg in &method_call_expr.args {
                walk_expr(arg, variables);
            }
        }
        Expr::FnCall(fn_call_expr, _) => {
            for arg in &fn_call_expr.args {
                walk_expr(arg, variables);
            }
        }
        Expr::And(expr_vec, _) | Expr::Or(expr_vec, _) | Expr::Coalesce(expr_vec, _) => {
            for e in &**expr_vec {
                walk_expr(e, variables);
            }
        }
        Expr::Array(expr_vec, _) | Expr::InterpolatedString(expr_vec, _) => {
            for e in expr_vec {
                walk_expr(e, variables);
            }
        }
        Expr::Map(map_data, _) => {
            for (_, value_expr) in &map_data.0 {
                walk_expr(value_expr, variables);
            }
        }
        Expr::Stmt(stmt_block) => {
            for s in stmt_block.statements() {
                walk_stmt(s, variables);
            }
        }
        Expr::Custom(custom_expr, _) => {
            for e in &custom_expr.inputs {
                walk_expr(e, variables);
            }
        }
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

/// Attempts to reconstruct a full variable path (e.g., "tx.value") from an expression.
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
    use super::*;
    use rhai::{Engine, ParseError};

    fn get_vars(script: &str) -> Result<HashSet<String>, ParseError> {
        let engine = Engine::new();
        let ast = engine.compile(script)?;
        Ok(get_accessed_variables(&ast))
    }

    #[test]
    fn test_simple_binary_op() {
        let vars = get_vars("tx.value > 100").unwrap();
        assert_eq!(vars, HashSet::from(["tx.value".to_string()]));
    }

    #[test]
    fn test_logical_operators() {
        let script = r#"tx.from == owner && log.name != "Transfer" || block.number > 1000"#;
        let vars = get_vars(script).unwrap();
        assert_eq!(
            vars,
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
        let vars = get_vars("tx.from ?? fallback_addr.address").unwrap();
        assert_eq!(
            vars,
            HashSet::from(["tx.from".to_string(), "fallback_addr.address".to_string(),])
        );
    }

    #[test]
    fn test_deeply_nested_variable() {
        let script = r#"log.params.level_one.level_two.user == "admin""#;
        let vars = get_vars(script).unwrap();
        assert_eq!(
            vars,
            HashSet::from(["log.params.level_one.level_two.user".to_string()])
        );
    }

    #[test]
    fn test_variables_in_function_calls() {
        let vars = get_vars("my_func(tx.value, log.params.user, 42)").unwrap();
        assert_eq!(
            vars,
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
        let vars = get_vars(script).unwrap();
        assert_eq!(
            vars,
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
        let vars = get_vars(script).unwrap();
        assert_eq!(
            vars,
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
    fn test_variables_in_string_or_comments_are_ignored() {
        let script = r#"
            // This is a comment about tx.value
            let x = "this string mentions log.name";
            tx.from == "0x123"
        "#;
        let vars = get_vars(script).unwrap();
        assert_eq!(vars, HashSet::from(["tx.from".to_string()]));
    }

    #[test]
    fn test_indexing_expression() {
        let script = r#"tx.logs[0].name == "Transfer" && some_array[tx.index] > 100"#;
        let vars = get_vars(script).unwrap();
        assert_eq!(
            vars,
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
        let vars = get_vars(script).unwrap();
        assert_eq!(
            vars,
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
        let vars = get_vars(script).unwrap();
        assert_eq!(
            vars,
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
        let script = "1 + 1 == 2";
        let vars = get_vars(script).unwrap();
        assert!(vars.is_empty());
    }

    #[test]
    fn test_array_and_map_literals() {
        let script = r#"
            let my_array = [tx.value, log.topic];
            let my_map = #{ a: some.value, b: 42 };
            my_array[0] > my_map.a
        "#;
        let vars = get_vars(script).unwrap();
        assert_eq!(
            vars,
            HashSet::from([
                "tx.value".to_string(),
                "log.topic".to_string(),
                "some.value".to_string(),
                "my_array".to_string(),
                "my_map.a".to_string(),
            ])
        );
    }
}
