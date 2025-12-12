use std::collections::HashMap;
use crate::token::Token;
use crate::ast::ASTNode;

pub struct Interpreter {
    variables: HashMap<String, i32>,
}

impl Interpreter {
    pub fn new() -> Self {
        Interpreter {
            variables: HashMap::new(),
        }
    }

    pub fn interpret(&mut self, node: &ASTNode) -> Result<i32, String> {
        match node {
            ASTNode::BinOp { left, op, right } => {
                let left_val = self.interpret(left)?;
                let right_val = self.interpret(right)?;
                match op {
                    Token::Plus => Ok(left_val + right_val),
                    Token::Minus => Ok(left_val - right_val),
                    Token::Multiply => Ok(left_val * right_val),
                    Token::Divide => {
                        if right_val == 0 {
                            Err("Division by zero".to_string())
                        } else {
                            Ok(left_val / right_val)
                        }
                    }
                    _ => Err(format!("Unknown binary operator: {:?}", op)),
                }
            }
            ASTNode::UnaryOp { op, expr } => {
                let val = self.interpret(expr)?;
                match op {
                    Token::Plus => Ok(val),
                    Token::Minus => Ok(-val),
                    _ => Err(format!("Unknown unary operator: {:?}", op)),
                }
            }
            ASTNode::Num(val) => Ok(*val),
            ASTNode::Var(name) => self
                .variables
                .get(name)
                .copied()
                .ok_or_else(|| format!("Undefined variable: {}", name)),
            ASTNode::Assign { var, expr } => {
                let val = self.interpret(expr)?;
                self.variables.insert(var.clone(), val);
                Ok(val)
            }
            ASTNode::Compound { children } => {
                let mut result = 0;
                for child in children {
                    result = self.interpret(child)?;
                }
                Ok(result)
            }
            ASTNode::NoOp => Ok(0),
        }
    }

    pub fn get_variables(&self) -> &HashMap<String, i32> {
        &self.variables
    }
}

impl Default for Interpreter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_num_node(val: i32) -> ASTNode {
        ASTNode::Num(val)
    }

    #[test]
    fn test_interpret_number() {
        let mut interp = Interpreter::new();
        let node = create_num_node(42);
        assert_eq!(interp.interpret(&node).unwrap(), 42);
    }

    #[test]
    fn test_interpret_addition() {
        let mut interp = Interpreter::new();
        let node = ASTNode::BinOp {
            left: Box::new(create_num_node(2)),
            op: Token::Plus,
            right: Box::new(create_num_node(3)),
        };
        assert_eq!(interp.interpret(&node).unwrap(), 5);
    }

    #[test]
    fn test_interpret_subtraction() {
        let mut interp = Interpreter::new();
        let node = ASTNode::BinOp {
            left: Box::new(create_num_node(10)),
            op: Token::Minus,
            right: Box::new(create_num_node(3)),
        };
        assert_eq!(interp.interpret(&node).unwrap(), 7);
    }

    #[test]
    fn test_interpret_multiplication() {
        let mut interp = Interpreter::new();
        let node = ASTNode::BinOp {
            left: Box::new(create_num_node(4)),
            op: Token::Multiply,
            right: Box::new(create_num_node(5)),
        };
        assert_eq!(interp.interpret(&node).unwrap(), 20);
    }

    #[test]
    fn test_interpret_division() {
        let mut interp = Interpreter::new();
        let node = ASTNode::BinOp {
            left: Box::new(create_num_node(20)),
            op: Token::Divide,
            right: Box::new(create_num_node(4)),
        };
        assert_eq!(interp.interpret(&node).unwrap(), 5);
    }

    #[test]
    fn test_interpret_division_by_zero() {
        let mut interp = Interpreter::new();
        let node = ASTNode::BinOp {
            left: Box::new(create_num_node(10)),
            op: Token::Divide,
            right: Box::new(create_num_node(0)),
        };
        assert!(interp.interpret(&node).is_err());
    }

    #[test]
    fn test_interpret_unary_minus() {
        let mut interp = Interpreter::new();
        let node = ASTNode::UnaryOp {
            op: Token::Minus,
            expr: Box::new(create_num_node(5)),
        };
        assert_eq!(interp.interpret(&node).unwrap(), -5);
    }

    #[test]
    fn test_interpret_unary_plus() {
        let mut interp = Interpreter::new();
        let node = ASTNode::UnaryOp {
            op: Token::Plus,
            expr: Box::new(create_num_node(5)),
        };
        assert_eq!(interp.interpret(&node).unwrap(), 5);
    }

    #[test]
    fn test_interpret_assignment() {
        let mut interp = Interpreter::new();
        let node = ASTNode::Assign {
            var: "x".to_string(),
            expr: Box::new(create_num_node(42)),
        };
        interp.interpret(&node).unwrap();
        assert_eq!(interp.get_variables().get("x"), Some(&42));
    }

    #[test]
    fn test_interpret_variable() {
        let mut interp = Interpreter::new();
        interp.variables.insert("x".to_string(), 42);
        let node = ASTNode::Var("x".to_string());
        assert_eq!(interp.interpret(&node).unwrap(), 42);
    }

    #[test]
    fn test_interpret_undefined_variable() {
        let mut interp = Interpreter::new();
        let node = ASTNode::Var("undefined".to_string());
        assert!(interp.interpret(&node).is_err());
    }

    #[test]
    fn test_interpret_compound() {
        let mut interp = Interpreter::new();
        let node = ASTNode::Compound {
            children: vec![
                ASTNode::Assign {
                    var: "x".to_string(),
                    expr: Box::new(create_num_node(5)),
                },
                ASTNode::Assign {
                    var: "y".to_string(),
                    expr: Box::new(create_num_node(10)),
                },
            ],
        };
        interp.interpret(&node).unwrap();
        assert_eq!(interp.get_variables().get("x"), Some(&5));
        assert_eq!(interp.get_variables().get("y"), Some(&10));
    }

    #[test]
    fn test_interpret_noop() {
        let mut interp = Interpreter::new();
        let node = ASTNode::NoOp;
        assert_eq!(interp.interpret(&node).unwrap(), 0);
    }
}
