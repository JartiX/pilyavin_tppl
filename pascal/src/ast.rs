use crate::token::Token;

#[derive(Debug, Clone)]
pub enum ASTNode {
    BinOp {
        left: Box<ASTNode>,
        op: Token,
        right: Box<ASTNode>,
    },
    UnaryOp {
        op: Token,
        expr: Box<ASTNode>,
    },
    Num(i32),
    Var(String),
    Assign {
        var: String,
        expr: Box<ASTNode>,
    },
    Compound {
        children: Vec<ASTNode>,
    },
    NoOp,
}