#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Integer(i32),
    Plus,
    Minus,
    Multiply,
    Divide,
    LParen,
    RParen,
    Begin,
    End,
    Semi,
    Dot,
    Assign,
    Id(String),
    Eof,
}