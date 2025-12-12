use pascal_interpreter::execute;

#[cfg(test)]
mod integration_tests {
    use super::*;

    #[test]
    fn test_simple_program() {
        let program = "BEGIN x := 5 END.";
        let result = execute(program).unwrap();
        assert_eq!(result.get("x"), Some(&5));
    }

    #[test]
    fn test_multiple_assignments() {
        let program = "BEGIN x := 5; y := 10; z := x + y END.";
        let result = execute(program).unwrap();
        assert_eq!(result.get("x"), Some(&5));
        assert_eq!(result.get("y"), Some(&10));
        assert_eq!(result.get("z"), Some(&15));
    }

    #[test]
    fn test_arithmetic_operations() {
        let program = "BEGIN a := 10; b := 3; sum := a + b; diff := a - b; prod := a * b; quot := a / b END.";
        let result = execute(program).unwrap();
        assert_eq!(result.get("sum"), Some(&13));
        assert_eq!(result.get("diff"), Some(&7));
        assert_eq!(result.get("prod"), Some(&30));
        assert_eq!(result.get("quot"), Some(&3));
    }

    #[test]
    fn test_expression_precedence() {
        let program = "BEGIN x := 2 + 3 * 4 END.";
        let result = execute(program).unwrap();
        assert_eq!(result.get("x"), Some(&14));
    }

    #[test]
    fn test_parentheses() {
        let program = "BEGIN x := (2 + 3) * 4 END.";
        let result = execute(program).unwrap();
        assert_eq!(result.get("x"), Some(&20));
    }

    #[test]
    fn test_unary_operators() {
        let program = "BEGIN x := -5; y := +10; z := -x END.";
        let result = execute(program).unwrap();
        assert_eq!(result.get("x"), Some(&-5));
        assert_eq!(result.get("y"), Some(&10));
        assert_eq!(result.get("z"), Some(&5));
    }

    #[test]
    fn test_nested_blocks() {
        let program = "BEGIN x := 5; BEGIN y := 10; z := x + y END END.";
        let result = execute(program).unwrap();
        assert_eq!(result.get("x"), Some(&5));
        assert_eq!(result.get("y"), Some(&10));
        assert_eq!(result.get("z"), Some(&15));
    }

    #[test]
    fn test_variable_reuse() {
        let program = "BEGIN x := 5; x := x + 1; x := x * 2 END.";
        let result = execute(program).unwrap();
        assert_eq!(result.get("x"), Some(&12));
    }

    #[test]
    fn test_empty_statements() {
        let program = "BEGIN x := 5; ; y := 10 END.";
        let result = execute(program).unwrap();
        assert_eq!(result.get("x"), Some(&5));
        assert_eq!(result.get("y"), Some(&10));
    }

    #[test]
    fn test_complex_expression() {
        let program = "BEGIN x := 7 + 3 * (10 / (12 / (3 + 1) - 1)) END.";
        let result = execute(program).unwrap();
        assert_eq!(result.get("x"), Some(&22));
    }

    #[test]
    fn test_undefined_variable_error() {
        let program = "BEGIN x := y + 1 END.";
        let result = execute(program);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Undefined variable"));
    }

    #[test]
    fn test_division_by_zero() {
        let program = "BEGIN x := 10 / 0 END.";
        let result = execute(program);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Division by zero"));
    }

    #[test]
    fn test_case_insensitive_keywords() {
        let program = "begin x := 5; y := 10 end.";
        let result = execute(program).unwrap();
        assert_eq!(result.get("x"), Some(&5));
        assert_eq!(result.get("y"), Some(&10));
    }

    #[test]
    fn test_whitespace_handling() {
        let program = "   BEGIN    x   :=   5   ;   y   :=   10   END   .   ";
        let result = execute(program).unwrap();
        assert_eq!(result.get("x"), Some(&5));
        assert_eq!(result.get("y"), Some(&10));
    }

    #[test]
    fn test_long_identifiers() {
        let program = "BEGIN very_long_variable_name := 100 END.";
        let result = execute(program).unwrap();
        assert_eq!(result.get("very_long_variable_name"), Some(&100));
    }

    #[test]
    fn test_nested_parentheses() {
        let program = "BEGIN x := ((2 + 3) * (4 + 5)) END.";
        let result = execute(program).unwrap();
        assert_eq!(result.get("x"), Some(&45));
    }

    #[test]
    fn test_multiple_nested_blocks() {
        let program = "BEGIN a := 1; BEGIN b := 2; BEGIN c := a + b END END END.";
        let result = execute(program).unwrap();
        assert_eq!(result.get("a"), Some(&1));
        assert_eq!(result.get("b"), Some(&2));
        assert_eq!(result.get("c"), Some(&3));
    }

    #[test]
    fn test_chained_operations() {
        let program = "BEGIN x := 1 + 2 + 3 + 4 + 5 END.";
        let result = execute(program).unwrap();
        assert_eq!(result.get("x"), Some(&15));
    }

    #[test]
    fn test_mixed_operations() {
        let program = "BEGIN x := 10 - 5 + 3 * 2 / 2 END.";
        let result = execute(program).unwrap();
        assert_eq!(result.get("x"), Some(&8));
    }
}