use crate::token::Token;

pub struct Lexer {
    text: Vec<char>,
    pos: usize,
    current_char: Option<char>,
}

impl Lexer {
    pub fn new(text: &str) -> Self {
        let chars: Vec<char> = text.chars().collect();
        let current_char = if chars.is_empty() { None } else { Some(chars[0]) };
        Lexer {
            text: chars,
            pos: 0,
            current_char,
        }
    }

    fn advance(&mut self) {
        self.pos += 1;
        if self.pos >= self.text.len() {
            self.current_char = None;
        } else {
            self.current_char = Some(self.text[self.pos]);
        }
    }

    fn peek(&self) -> Option<char> {
        let peek_pos = self.pos + 1;
        if peek_pos >= self.text.len() {
            None
        } else {
            Some(self.text[peek_pos])
        }
    }

    fn skip_whitespace(&mut self) {
        while let Some(ch) = self.current_char {
            if ch.is_whitespace() {
                self.advance();
            } else {
                break;
            }
        }
    }

    fn integer(&mut self) -> i32 {
        let mut result = String::new();
        while let Some(ch) = self.current_char {
            if ch.is_ascii_digit() {
                result.push(ch);
                self.advance();
            } else {
                break;
            }
        }
        result.parse().unwrap()
    }

    fn id(&mut self) -> String {
        let mut result = String::new();
        while let Some(ch) = self.current_char {
            if ch.is_alphanumeric() || ch == '_' {
                result.push(ch);
                self.advance();
            } else {
                break;
            }
        }
        result
    }

    pub fn get_next_token(&mut self) -> Result<Token, String> {
        while let Some(ch) = self.current_char {
            if ch.is_whitespace() {
                self.skip_whitespace();
                continue;
            }

            if ch.is_ascii_digit() {
                return Ok(Token::Integer(self.integer()));
            }

            if ch.is_alphabetic() {
                let id = self.id();
                let token = match id.to_uppercase().as_str() {
                    "BEGIN" => Token::Begin,
                    "END" => Token::End,
                    _ => Token::Id(id),
                };
                return Ok(token);
            }

            if ch == ':' && self.peek() == Some('=') {
                self.advance();
                self.advance();
                return Ok(Token::Assign);
            }

            let token = match ch {
                '+' => Token::Plus,
                '-' => Token::Minus,
                '*' => Token::Multiply,
                '/' => Token::Divide,
                '(' => Token::LParen,
                ')' => Token::RParen,
                ';' => Token::Semi,
                '.' => Token::Dot,
                _ => return Err(format!("Invalid character: {}", ch)),
            };

            self.advance();
            return Ok(token);
        }

        Ok(Token::Eof)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_integer_token() {
        let mut lexer = Lexer::new("123");
        assert_eq!(lexer.get_next_token().unwrap(), Token::Integer(123));
    }

    #[test]
    fn test_operators() {
        let mut lexer = Lexer::new("+ - * /");
        assert_eq!(lexer.get_next_token().unwrap(), Token::Plus);
        assert_eq!(lexer.get_next_token().unwrap(), Token::Minus);
        assert_eq!(lexer.get_next_token().unwrap(), Token::Multiply);
        assert_eq!(lexer.get_next_token().unwrap(), Token::Divide);
    }

    #[test]
    fn test_keywords() {
        let mut lexer = Lexer::new("BEGIN END begin end");
        assert_eq!(lexer.get_next_token().unwrap(), Token::Begin);
        assert_eq!(lexer.get_next_token().unwrap(), Token::End);
        assert_eq!(lexer.get_next_token().unwrap(), Token::Begin);
        assert_eq!(lexer.get_next_token().unwrap(), Token::End);
    }

    #[test]
    fn test_assignment() {
        let mut lexer = Lexer::new("x := 5");
        assert_eq!(lexer.get_next_token().unwrap(), Token::Id("x".to_string()));
        assert_eq!(lexer.get_next_token().unwrap(), Token::Assign);
        assert_eq!(lexer.get_next_token().unwrap(), Token::Integer(5));
    }

    #[test]
    fn test_parentheses() {
        let mut lexer = Lexer::new("( )");
        assert_eq!(lexer.get_next_token().unwrap(), Token::LParen);
        assert_eq!(lexer.get_next_token().unwrap(), Token::RParen);
    }

    #[test]
    fn test_semicolon_and_dot() {
        let mut lexer = Lexer::new("; .");
        assert_eq!(lexer.get_next_token().unwrap(), Token::Semi);
        assert_eq!(lexer.get_next_token().unwrap(), Token::Dot);
    }

    #[test]
    fn test_identifier() {
        let mut lexer = Lexer::new("variable_name x123");
        assert_eq!(lexer.get_next_token().unwrap(), Token::Id("variable_name".to_string()));
        assert_eq!(lexer.get_next_token().unwrap(), Token::Id("x123".to_string()));
    }

    #[test]
    fn test_invalid_character() {
        let mut lexer = Lexer::new("@");
        assert!(lexer.get_next_token().is_err());
    }

    #[test]
    fn test_whitespace_handling() {
        let mut lexer = Lexer::new("  \t\n  123  \n ");
        assert_eq!(lexer.get_next_token().unwrap(), Token::Integer(123));
    }
}