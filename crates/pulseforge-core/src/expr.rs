//! A small, dependency-free expression evaluator.
//!
//! The DAW lets the user type formulas (for automation shapes and quick
//! experiments), so the parser lives in the engine crate where it is usable
//! from the UI, from tests and later from the audio path without pulling in a
//! scripting runtime.
//!
//! Grammar, lowest precedence first:
//!
//! ```text
//! expression := term (('+' | '-') term)*
//! term       := unary (('*' | '/' | '%') unary)*
//! unary      := ('-' | '+') unary | power
//! power      := primary ('^' unary)?          // right associative
//! primary    := number | 'x' | constant
//!             | function '(' expression ')'
//!             | '(' expression ')'
//! ```
//!
//! Supported functions are `exp`, `ln`, `sqrt`, `abs`, `floor`, `round`, `sin`,
//! `cos` and `tan` (radians); constants are `pi` and `e`. The only variable is
//! `x`, which the caller supplies (for automation it is the position inside the
//! lane, typically 0..=1 or 0..=length in steps).
//!
//! ```
//! use pulseforge_core::expr::Expression;
//! let expression = Expression::parse("2 + 3 * 4").unwrap();
//! assert_eq!(expression.evaluate(0.0), 14.0);
//! ```

use std::fmt;

/// One parsed formula.
#[derive(Clone, Debug)]
pub struct Expression {
    root: Node,
}

/// Why a formula could not be parsed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExprError {
    pub message: String,
    /// Byte-ish index (in characters) of the offending token.
    pub position: usize,
}

impl fmt::Display for ExprError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} at {}", self.message, self.position)
    }
}

impl std::error::Error for ExprError {}

#[derive(Clone, Copy, Debug)]
enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Pow,
}

#[derive(Clone, Copy, Debug)]
enum Function {
    Exp,
    Ln,
    Sqrt,
    Abs,
    Floor,
    Round,
    Sin,
    Cos,
    Tan,
}

impl Function {
    fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "exp" => Self::Exp,
            "ln" => Self::Ln,
            "sqrt" => Self::Sqrt,
            "abs" => Self::Abs,
            "floor" => Self::Floor,
            "round" => Self::Round,
            "sin" => Self::Sin,
            "cos" => Self::Cos,
            "tan" => Self::Tan,
            _ => return None,
        })
    }

    fn apply(self, value: f64) -> f64 {
        match self {
            Self::Exp => value.exp(),
            Self::Ln => value.ln(),
            Self::Sqrt => value.sqrt(),
            Self::Abs => value.abs(),
            Self::Floor => value.floor(),
            Self::Round => value.round(),
            Self::Sin => value.sin(),
            Self::Cos => value.cos(),
            Self::Tan => value.tan(),
        }
    }
}

#[derive(Clone, Debug)]
enum Node {
    Number(f64),
    Variable,
    Neg(Box<Node>),
    Binary(BinOp, Box<Node>, Box<Node>),
    Call(Function, Box<Node>),
}

impl Expression {
    /// Parses `source`. Whitespace is ignored; anything else must be part of
    /// the grammar, so typos are reported instead of silently ignored.
    pub fn parse(source: &str) -> Result<Self, ExprError> {
        let mut parser = Parser {
            chars: source.chars().collect(),
            index: 0,
        };
        let root = parser.expression()?;
        parser.skip_whitespace();
        if parser.index < parser.chars.len() {
            return Err(parser.error("unexpected input"));
        }
        Ok(Self { root })
    }

    /// Evaluates the formula for one value of `x`.
    ///
    /// Division by zero and out-of-domain inputs follow IEEE-754 (`inf`, `NaN`)
    /// rather than failing, so a caller can clamp the result.
    pub fn evaluate(&self, x: f64) -> f64 {
        evaluate(&self.root, x)
    }
}

fn evaluate(node: &Node, x: f64) -> f64 {
    match node {
        Node::Number(value) => *value,
        Node::Variable => x,
        Node::Neg(inner) => -evaluate(inner, x),
        Node::Binary(op, left, right) => {
            let left = evaluate(left, x);
            let right = evaluate(right, x);
            match op {
                BinOp::Add => left + right,
                BinOp::Sub => left - right,
                BinOp::Mul => left * right,
                BinOp::Div => left / right,
                BinOp::Rem => left % right,
                BinOp::Pow => left.powf(right),
            }
        }
        Node::Call(function, argument) => function.apply(evaluate(argument, x)),
    }
}

struct Parser {
    chars: Vec<char>,
    index: usize,
}

impl Parser {
    fn error(&self, message: &str) -> ExprError {
        ExprError {
            message: message.to_owned(),
            position: self.index,
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.index).copied()
    }

    fn skip_whitespace(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.index += 1;
        }
    }

    fn eat(&mut self, expected: char) -> bool {
        self.skip_whitespace();
        if self.peek() == Some(expected) {
            self.index += 1;
            true
        } else {
            false
        }
    }

    fn expression(&mut self) -> Result<Node, ExprError> {
        let mut node = self.term()?;
        loop {
            self.skip_whitespace();
            let op = match self.peek() {
                Some('+') => BinOp::Add,
                Some('-') => BinOp::Sub,
                _ => return Ok(node),
            };
            self.index += 1;
            let right = self.term()?;
            node = Node::Binary(op, Box::new(node), Box::new(right));
        }
    }

    fn term(&mut self) -> Result<Node, ExprError> {
        let mut node = self.unary()?;
        loop {
            self.skip_whitespace();
            let op = match self.peek() {
                Some('*') => BinOp::Mul,
                Some('/') => BinOp::Div,
                Some('%') => BinOp::Rem,
                _ => return Ok(node),
            };
            self.index += 1;
            let right = self.unary()?;
            node = Node::Binary(op, Box::new(node), Box::new(right));
        }
    }

    fn unary(&mut self) -> Result<Node, ExprError> {
        self.skip_whitespace();
        match self.peek() {
            Some('-') => {
                self.index += 1;
                Ok(Node::Neg(Box::new(self.unary()?)))
            }
            Some('+') => {
                self.index += 1;
                self.unary()
            }
            _ => self.power(),
        }
    }

    fn power(&mut self) -> Result<Node, ExprError> {
        let base = self.primary()?;
        self.skip_whitespace();
        if self.peek() == Some('^') {
            self.index += 1;
            // Right associative, and the exponent may be negative.
            let exponent = self.unary()?;
            return Ok(Node::Binary(BinOp::Pow, Box::new(base), Box::new(exponent)));
        }
        Ok(base)
    }

    fn primary(&mut self) -> Result<Node, ExprError> {
        self.skip_whitespace();
        let Some(current) = self.peek() else {
            return Err(self.error("unexpected end of formula"));
        };
        if current == '(' {
            self.index += 1;
            let inner = self.expression()?;
            if !self.eat(')') {
                return Err(self.error("missing ')'"));
            }
            return Ok(inner);
        }
        if current.is_ascii_digit() || current == '.' {
            return self.number();
        }
        if current.is_alphabetic() {
            let start = self.index;
            let mut name = String::new();
            while self
                .peek()
                .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
            {
                name.push(self.chars[self.index]);
                self.index += 1;
            }
            match name.as_str() {
                "x" => return Ok(Node::Variable),
                "pi" => return Ok(Node::Number(std::f64::consts::PI)),
                "e" => return Ok(Node::Number(std::f64::consts::E)),
                _ => {}
            }
            let Some(function) = Function::from_name(&name) else {
                self.index = start;
                return Err(self.error(&format!("unknown name '{name}'")));
            };
            if !self.eat('(') {
                return Err(self.error("expected '(' after function name"));
            }
            let argument = self.expression()?;
            if !self.eat(')') {
                return Err(self.error("missing ')'"));
            }
            return Ok(Node::Call(function, Box::new(argument)));
        }
        Err(self.error("unexpected character"))
    }

    fn number(&mut self) -> Result<Node, ExprError> {
        let start = self.index;
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.index += 1;
        }
        if self.peek() == Some('.') {
            self.index += 1;
            while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                self.index += 1;
            }
        }
        let text: String = self.chars[start..self.index].iter().collect();
        text.parse::<f64>()
            .map(Node::Number)
            .map_err(|_| ExprError {
                message: format!("invalid number '{text}'"),
                position: start,
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(source: &str, x: f64) -> f64 {
        Expression::parse(source)
            .unwrap_or_else(|error| panic!("{source}: {error}"))
            .evaluate(x)
    }

    #[test]
    fn arithmetic_follows_precedence() {
        assert_eq!(value("2 + 3 * 4", 0.0), 14.0);
        assert_eq!(value("(1 + 2) * 3", 0.0), 9.0);
        assert_eq!(value("10 % 3", 0.0), 1.0);
        assert_eq!(value("7 / 2", 0.0), 3.5);
    }

    #[test]
    fn power_is_right_associative_and_binds_above_unary_minus() {
        assert_eq!(value("2 ^ 3 ^ 2", 0.0), 512.0);
        assert_eq!(value("-3 ^ 2", 0.0), -9.0);
        assert_eq!(value("2 ^ -1", 0.0), 0.5);
    }

    #[test]
    fn functions_and_constants() {
        assert!((value("sin(pi / 2)", 0.0) - 1.0).abs() < 1e-12);
        assert!((value("cos(0) - 1", 0.0)).abs() < 1e-12);
        assert_eq!(value("sqrt(16)", 0.0), 4.0);
        assert!((value("exp(1) / e", 0.0) - 1.0).abs() < 1e-12);
        assert_eq!(value("abs(-4) + round(2.6)", 0.0), 7.0);
        assert_eq!(value("floor(-1.2)", 0.0), -2.0);
        assert!((value("ln(e)", 0.0) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn the_variable_is_substituted() {
        assert_eq!(value("x * 2 + 1", 3.0), 7.0);
        assert_eq!(value("tan(x)", 0.0), 0.0);
    }

    #[test]
    fn out_of_domain_results_are_inf_or_nan() {
        assert!(value("1 / 0", 0.0).is_infinite());
        assert!(value("sqrt(-1)", 0.0).is_nan());
    }

    #[test]
    fn syntax_errors_are_reported_with_a_position() {
        let error = Expression::parse("2 + * 3").unwrap_err();
        assert!(error.position > 0, "{error}");
        let error = Expression::parse("(1 + 2").unwrap_err();
        assert!(error.message.contains("missing ')'"), "{error}");
        let error = Expression::parse("nope(1)").unwrap_err();
        assert!(error.message.contains("unknown name"), "{error}");
        let error = Expression::parse("1 2").unwrap_err();
        assert!(error.message.contains("unexpected input"), "{error}");
        assert!(Expression::parse("   ").is_err());
    }
}
