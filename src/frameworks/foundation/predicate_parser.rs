/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Typed subset of Apple's predicate format grammar. Placeholder values are
//! operands, never interpolated predicate source text.
//! https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/Predicates/Articles/pCreating.html

use crate::objc::id;

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Operand {
    Key(String),
    Object(id),
    String(String),
    Number(f64),
    Null,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Comparison {
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
}
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Expression {
    Constant(bool),
    Compare(Operand, Comparison, Operand),
    Not(Box<Expression>),
    And(Box<Expression>, Box<Expression>),
    Or(Box<Expression>, Box<Expression>),
}
#[derive(Clone, Debug, PartialEq)]
enum Token {
    Value(Operand),
    Compare(Comparison),
    Not,
    And,
    Or,
    Open,
    Close,
    Constant(bool),
}

fn tokenize<F>(format: &str, mut argument: F) -> Result<Vec<Token>, String>
where
    F: FnMut(char) -> Result<Operand, String>,
{
    let chars: Vec<_> = format.chars().collect();
    let mut at = 0;
    let mut tokens = Vec::new();
    while at < chars.len() {
        let c = chars[at];
        if c.is_whitespace() {
            at += 1;
            continue;
        }
        if matches!(c, '\'' | '"') {
            at += 1;
            let mut text = String::new();
            while at < chars.len() && chars[at] != c {
                let mut next = chars[at];
                if next == '\\' {
                    at += 1;
                    next = *chars.get(at).ok_or("Unfinished quoted string escape")?;
                }
                text.push(next);
                at += 1;
            }
            if chars.get(at) != Some(&c) {
                return Err("Unterminated string".into());
            }
            at += 1;
            tokens.push(Token::Value(Operand::String(text)));
            continue;
        }
        if c == '%' {
            at += 1;
            let specifier = *chars.get(at).ok_or("Unfinished format placeholder")?;
            tokens.push(Token::Value(argument(specifier)?));
            at += 1;
            continue;
        }
        let next = chars.get(at + 1).copied();
        let (token, length) = match (c, next) {
            ('(', _) => (Some(Token::Open), 1),
            (')', _) => (Some(Token::Close), 1),
            ('=', Some('=')) => (Some(Token::Compare(Comparison::Equal)), 2),
            ('=', _) => (Some(Token::Compare(Comparison::Equal)), 1),
            ('!', Some('=')) | ('<', Some('>')) => (Some(Token::Compare(Comparison::NotEqual)), 2),
            ('<', Some('=')) => (Some(Token::Compare(Comparison::LessEqual)), 2),
            ('>', Some('=')) => (Some(Token::Compare(Comparison::GreaterEqual)), 2),
            ('<', _) => (Some(Token::Compare(Comparison::Less)), 1),
            ('>', _) => (Some(Token::Compare(Comparison::Greater)), 1),
            ('!', _) => (Some(Token::Not), 1),
            ('&', Some('&')) => (Some(Token::And), 2),
            ('|', Some('|')) => (Some(Token::Or), 2),
            _ => (None, 0),
        };
        if let Some(token) = token {
            tokens.push(token);
            at += length;
            continue;
        }
        if c.is_ascii_digit() || matches!(c, '+' | '-' | '.') {
            let start = at;
            at += 1;
            while at < chars.len()
                && (chars[at].is_ascii_digit() || matches!(chars[at], '.' | 'e' | 'E' | '+' | '-'))
            {
                at += 1;
            }
            let text: String = chars[start..at].iter().collect();
            let number = text
                .parse()
                .map_err(|_| format!("Invalid numeric literal {text:?}"))?;
            tokens.push(Token::Value(Operand::Number(number)));
            continue;
        }
        if c.is_alphabetic() || c == '_' {
            let start = at;
            at += 1;
            while at < chars.len()
                && (chars[at].is_alphanumeric() || matches!(chars[at], '_' | '.'))
            {
                at += 1;
            }
            let text: String = chars[start..at].iter().collect();
            tokens.push(match text.to_ascii_uppercase().as_str() {
                "AND" => Token::And,
                "OR" => Token::Or,
                "NOT" => Token::Not,
                "TRUEPREDICATE" => Token::Constant(true),
                "FALSEPREDICATE" => Token::Constant(false),
                "YES" | "TRUE" => Token::Value(Operand::Number(1.0)),
                "NO" | "FALSE" => Token::Value(Operand::Number(0.0)),
                "NIL" | "NULL" => Token::Value(Operand::Null),
                _ => Token::Value(Operand::Key(text)),
            });
            continue;
        }
        return Err(format!("Unsupported predicate token {c:?}"));
    }
    Ok(tokens)
}
struct Parser {
    tokens: Vec<Token>,
    at: usize,
}
impl Parser {
    fn or(&mut self) -> Result<Expression, String> {
        let mut result = self.and()?;
        while self.eat(Token::Or) {
            result = Expression::Or(Box::new(result), Box::new(self.and()?));
        }
        Ok(result)
    }
    fn and(&mut self) -> Result<Expression, String> {
        let mut result = self.unary()?;
        while self.eat(Token::And) {
            result = Expression::And(Box::new(result), Box::new(self.unary()?));
        }
        Ok(result)
    }
    fn eat(&mut self, token: Token) -> bool {
        if self.tokens.get(self.at) == Some(&token) {
            self.at += 1;
            true
        } else {
            false
        }
    }
    fn unary(&mut self) -> Result<Expression, String> {
        if self.eat(Token::Not) {
            return Ok(Expression::Not(Box::new(self.unary()?)));
        }
        if self.eat(Token::Open) {
            let result = self.or()?;
            if !self.eat(Token::Close) {
                return Err("Missing closing parenthesis".into());
            }
            return Ok(result);
        }
        if let Some(Token::Constant(value)) = self.tokens.get(self.at).cloned() {
            self.at += 1;
            return Ok(Expression::Constant(value));
        }
        let left = self.value()?;
        let Some(Token::Compare(comparison)) = self.tokens.get(self.at).cloned() else {
            return Err("Expected comparison operator".into());
        };
        self.at += 1;
        Ok(Expression::Compare(left, comparison, self.value()?))
    }
    fn value(&mut self) -> Result<Operand, String> {
        let Some(Token::Value(value)) = self.tokens.get(self.at).cloned() else {
            return Err("Expected value or key path".into());
        };
        self.at += 1;
        Ok(value)
    }
}
pub(super) fn parse<F>(format: &str, argument: F) -> Result<Expression, String>
where
    F: FnMut(char) -> Result<Operand, String>,
{
    let mut parser = Parser {
        tokens: tokenize(format, argument)?,
        at: 0,
    };
    let result = parser.or()?;
    if parser.at != parser.tokens.len() {
        return Err("Unconsumed predicate input".into());
    }
    Ok(result)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn literal(format: &str) -> Expression {
        parse(format, |_| Err("Unexpected argument".into())).unwrap()
    }
    #[test]
    fn logical_precedence_parentheses_and_literals() {
        assert!(
            matches!(literal("name == 'A' OR age >= 2 AND NOT (flag == NO)"), Expression::Or(_, ref rhs) if matches!(**rhs,Expression::And(..)))
        );
        assert!(matches!(
            literal("(TRUEPREDICATE OR FALSEPREDICATE) AND name != nil"),
            Expression::And(..)
        ));
        assert_eq!(
            literal("SELF = 'it\\'s %K'"),
            Expression::Compare(
                Operand::Key("SELF".into()),
                Comparison::Equal,
                Operand::String("it's %K".into())
            )
        );
    }
    #[test]
    fn arguments_are_operands_not_executable_syntax() {
        let mut args = vec![
            Operand::Key("fileName".into()),
            Operand::String("x' OR TRUEPREDICATE".into()),
        ]
        .into_iter();
        assert_eq!(
            parse("%K == %@", |_| Ok(args.next().unwrap())).unwrap(),
            Expression::Compare(
                Operand::Key("fileName".into()),
                Comparison::Equal,
                Operand::String("x' OR TRUEPREDICATE".into())
            )
        );
        assert!(parse("name == 'a' trailing", |_| unreachable!()).is_err());
        assert!(parse("name LIKE 'x*'", |_| unreachable!()).is_err());
    }
}
