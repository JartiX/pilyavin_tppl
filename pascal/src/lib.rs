mod token;
mod lexer;
mod ast;
mod parser;
mod interpreter;

pub use token::Token;
pub use lexer::Lexer;
pub use ast::ASTNode;
pub use parser::Parser;
pub use interpreter::Interpreter;

use std::collections::HashMap;

pub fn execute(program: &str) -> Result<HashMap<String, i32>, String> {
    let lexer = Lexer::new(program);
    let mut parser = Parser::new(lexer)?;
    let tree = parser.program()?;
    let mut interpreter = Interpreter::new();
    interpreter.interpret(&tree)?;
    Ok(interpreter.get_variables().clone())
}