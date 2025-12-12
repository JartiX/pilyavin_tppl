use crate::token::Token;
use crate::lexer::Lexer;
use crate::ast::ASTNode;

pub struct Parser {
    lexer: Lexer,
    current_token: Token,
}

impl Parser {
    pub fn new(mut lexer: Lexer) -> Result<Self, String> {
        let current_token = lexer.get_next_token()?;
        Ok(Parser {
            lexer,
            current_token,
        })
    }

    fn eat(&mut self, token_type: Token) -> Result<(), String> {
        if std::mem::discriminant(&self.current_token) == std::mem::discriminant(&token_type) {
            self.current_token = self.lexer.get_next_token()?;
            Ok(())
        } else {
            Err(format!(
                "Expected {:?}, got {:?}",
                token_type, self.current_token
            ))
        }
    }

    pub fn program(&mut self) -> Result<ASTNode, String> {
        let node = self.complex_statement()?;
        self.eat(Token::Dot)?;
        Ok(node)
    }

    fn complex_statement(&mut self) -> Result<ASTNode, String> {
        self.eat(Token::Begin)?;
        let nodes = self.statement_list()?;
        self.eat(Token::End)?;
        Ok(ASTNode::Compound { children: nodes })
    }

    fn statement_list(&mut self) -> Result<Vec<ASTNode>, String> {
        let mut results = vec![self.statement()?];

        while self.current_token == Token::Semi {
            self.eat(Token::Semi)?;
            results.push(self.statement()?);
        }

        Ok(results)
    }

    fn statement(&mut self) -> Result<ASTNode, String> {
        match &self.current_token {
            Token::Begin => self.complex_statement(),
            Token::Id(_) => self.assignment(),
            _ => Ok(self.empty()),
        }
    }

    fn assignment(&mut self) -> Result<ASTNode, String> {
        let var = self.variable()?;
        self.eat(Token::Assign)?;
        let expr = self.expr()?;
        Ok(ASTNode::Assign {
            var,
            expr: Box::new(expr),
        })
    }

    fn variable(&mut self) -> Result<String, String> {
        if let Token::Id(name) = &self.current_token {
            let name = name.clone();
            self.eat(Token::Id(String::new()))?;
            Ok(name)
        } else {
            Err(format!("Expected identifier, got {:?}", self.current_token))
        }
    }

    fn empty(&self) -> ASTNode {
        ASTNode::NoOp
    }

    fn expr(&mut self) -> Result<ASTNode, String> {
        let mut node = self.term()?;

        while matches!(self.current_token, Token::Plus | Token::Minus) {
            let op = self.current_token.clone();
            match op {
                Token::Plus => self.eat(Token::Plus)?,
                Token::Minus => self.eat(Token::Minus)?,
                _ => unreachable!(),
            }
            node = ASTNode::BinOp {
                left: Box::new(node),
                op,
                right: Box::new(self.term()?),
            };
        }

        Ok(node)
    }

    fn term(&mut self) -> Result<ASTNode, String> {
        let mut node = self.factor()?;

        while matches!(self.current_token, Token::Multiply | Token::Divide) {
            let op = self.current_token.clone();
            match op {
                Token::Multiply => self.eat(Token::Multiply)?,
                Token::Divide => self.eat(Token::Divide)?,
                _ => unreachable!(),
            }
            node = ASTNode::BinOp {
                left: Box::new(node),
                op,
                right: Box::new(self.factor()?),
            };
        }

        Ok(node)
    }

    fn factor(&mut self) -> Result<ASTNode, String> {
        let token = self.current_token.clone();

        match token {
            Token::Plus => {
                self.eat(Token::Plus)?;
                Ok(ASTNode::UnaryOp {
                    op: Token::Plus,
                    expr: Box::new(self.factor()?),
                })
            }
            Token::Minus => {
                self.eat(Token::Minus)?;
                Ok(ASTNode::UnaryOp {
                    op: Token::Minus,
                    expr: Box::new(self.factor()?),
                })
            }
            Token::Integer(val) => {
                self.eat(Token::Integer(0))?;
                Ok(ASTNode::Num(val))
            }
            Token::LParen => {
                self.eat(Token::LParen)?;
                let node = self.expr()?;
                self.eat(Token::RParen)?;
                Ok(node)
            }
            Token::Id(_) => {
                let var = self.variable()?;
                Ok(ASTNode::Var(var))
            }
            _ => Err(format!("Unexpected token in factor: {:?}", token)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_assignment() {
        let lexer = Lexer::new("BEGIN x := 5 END.");
        let mut parser = Parser::new(lexer).unwrap();
        let ast = parser.program();
        assert!(ast.is_ok());
    }

    #[test]
    fn test_parse_expression() {
        let lexer = Lexer::new("BEGIN x := 2 + 3 * 4 END.");
        let mut parser = Parser::new(lexer).unwrap();
        let ast = parser.program();
        assert!(ast.is_ok());
    }

    #[test]
    fn test_parse_parentheses() {
        let lexer = Lexer::new("BEGIN x := (2 + 3) * 4 END.");
        let mut parser = Parser::new(lexer).unwrap();
        let ast = parser.program();
        assert!(ast.is_ok());
    }

    #[test]
    fn test_parse_multiple_statements() {
        let lexer = Lexer::new("BEGIN x := 5; y := 10 END.");
        let mut parser = Parser::new(lexer).unwrap();
        let ast = parser.program();
        assert!(ast.is_ok());
    }

    #[test]
    fn test_parse_nested_blocks() {
        let lexer = Lexer::new("BEGIN x := 5; BEGIN y := 10 END END.");
        let mut parser = Parser::new(lexer).unwrap();
        let ast = parser.program();
        assert!(ast.is_ok());
    }

    #[test]
    fn test_parse_unary_operators() {
        let lexer = Lexer::new("BEGIN x := -5; y := +10 END.");
        let mut parser = Parser::new(lexer).unwrap();
        let ast = parser.program();
        assert!(ast.is_ok());
    }

    #[test]
    fn test_parse_missing_dot() {
        let lexer = Lexer::new("BEGIN x := 5 END");
        let mut parser = Parser::new(lexer).unwrap();
        let ast = parser.program();
        assert!(ast.is_err());
    }

    #[test]
    fn test_parse_missing_end() {
        let lexer = Lexer::new("BEGIN x := 5.");
        let mut parser = Parser::new(lexer).unwrap();
        let ast = parser.program();
        assert!(ast.is_err());
    }
}
