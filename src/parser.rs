//! Stage 2: syntax analysis.
//!
//! Grammar (every statement except `tap` ends with the `*` delimiter):
//!
//! ```text
//! program     := statement*
//! statement   := tap | declaration | command | "{" | "}"
//! tap         := "tap" (LIBRARY | GUILD | "wastes") "(" NUM ")" "*"?
//! declaration := TYPE ID ( "->" (STRING | NUM) )? "*"
//! command     := COMMAND ( ID | "(" (STRING | NUM | ID) ")" ) "*"
//! ```
//!
//! A missing `*` is a recoverable error: the parser reports it, inserts the
//! delimiter and keeps going (phrase-level recovery). Other syntax errors use
//! panic-mode recovery: skip ahead to the next statement and continue, so one
//! mistake doesn't hide the rest.

use crate::lexer::{Token, TokenKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VarType {
    Mana,
    HybMana,
    Scroll,
    Status,
}

impl VarType {
    pub fn from_keyword(s: &str) -> Option<VarType> {
        match s {
            "mana" => Some(VarType::Mana),
            "hybmana" => Some(VarType::HybMana),
            "scroll" => Some(VarType::Scroll),
            "status" => Some(VarType::Status),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            VarType::Mana => "mana",
            VarType::HybMana => "hybmana",
            VarType::Scroll => "scroll",
            VarType::Status => "status",
        }
    }

    pub fn is_numeric(self) -> bool {
        matches!(self, VarType::Mana | VarType::HybMana)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    Str(String),
    Num(String),
}

impl Literal {
    pub fn text(&self) -> &str {
        match self {
            Literal::Str(s) | Literal::Num(s) => s,
        }
    }

    fn kind_name(&self) -> &'static str {
        match self {
            Literal::Str(_) => "STRING",
            Literal::Num(_) => "NUM",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Arg {
    Var(String),
    Lit(Literal),
}

impl Arg {
    fn kind_name(&self) -> &'static str {
        match self {
            Arg::Var(_) => "ID",
            Arg::Lit(l) => l.kind_name(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Tap {
        source: String,
        count: u32,
        line: usize,
    },
    Declare {
        ty: VarType,
        name: String,
        value: Option<Literal>,
        scope: usize,
        line: usize,
    },
    Command {
        cmd: String,
        arg: Arg,
        scope: usize,
        line: usize,
    },
}

impl Stmt {
    /// The statement written back out as Lotus code (used in logs).
    pub fn code(&self) -> String {
        match self {
            Stmt::Tap { source, count, .. } => format!("tap {source}({count})"),
            Stmt::Declare {
                ty, name, value, ..
            } => match value {
                Some(Literal::Str(s)) => format!("{} {name} -> \"{s}\"", ty.name()),
                Some(Literal::Num(n)) => format!("{} {name} -> {n}", ty.name()),
                None => format!("{} {name}", ty.name()),
            },
            Stmt::Command { cmd, arg, .. } => match arg {
                Arg::Var(v) => format!("{cmd}({v})"),
                Arg::Lit(Literal::Str(s)) => format!("{cmd}(\"{s}\")"),
                Arg::Lit(Literal::Num(n)) => format!("{cmd}({n})"),
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Diagnostic {
    pub line: usize,
    pub message: String,
}

#[derive(Debug, Default)]
pub struct ParseResult {
    pub statements: Vec<Stmt>,
    /// Errors that stop the program from running.
    pub errors: Vec<Diagnostic>,
    /// Errors the parser fixed by itself (missing `*`).
    pub recoveries: Vec<Diagnostic>,
    /// Human-readable trace of each rule the parser matched.
    pub log: Vec<String>,
}

/// Brace and parenthesis balance check, run before parsing.
pub fn check_brackets(tokens: &[Token]) -> Result<(), Diagnostic> {
    let mut braces: Vec<&Token> = Vec::new();
    let mut parens: Vec<&Token> = Vec::new();
    for t in tokens {
        match t.kind {
            TokenKind::LBrace => braces.push(t),
            TokenKind::LParen => parens.push(t),
            TokenKind::RBrace => {
                if braces.pop().is_none() {
                    return Err(Diagnostic {
                        line: t.line,
                        message: "Unexpected '}' with no matching '{'".into(),
                    });
                }
            }
            TokenKind::RParen if parens.pop().is_none() => {
                return Err(Diagnostic {
                    line: t.line,
                    message: "Unexpected ')' with no matching '('".into(),
                });
            }
            _ => {}
        }
    }
    if let Some(open) = braces.first() {
        return Err(Diagnostic {
            line: open.line,
            message: format!("{} unclosed '{{' - missing '}}'", braces.len()),
        });
    }
    if let Some(open) = parens.first() {
        return Err(Diagnostic {
            line: open.line,
            message: format!("{} unclosed '(' - missing ')'", parens.len()),
        });
    }
    Ok(())
}

struct Parser<'a> {
    tokens: &'a [Token],
    pos: usize,
    scope: usize,
    out: ParseResult,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<&'a Token> {
        self.tokens.get(self.pos)
    }

    fn peek_kind(&self) -> Option<TokenKind> {
        self.peek().map(|t| t.kind)
    }

    fn advance(&mut self) -> Option<&'a Token> {
        let t = self.tokens.get(self.pos);
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    fn eat(&mut self, kind: TokenKind) -> Option<&'a Token> {
        if self.peek_kind() == Some(kind) {
            self.advance()
        } else {
            None
        }
    }

    fn error(&mut self, line: usize, message: impl Into<String>) {
        self.out.errors.push(Diagnostic {
            line,
            message: message.into(),
        });
    }

    /// Panic-mode recovery: skip to just after the next `*`, or stop before
    /// the next token that can start a statement.
    fn synchronize(&mut self) {
        while let Some(t) = self.peek() {
            match t.kind {
                TokenKind::Delim => {
                    self.pos += 1;
                    return;
                }
                TokenKind::Invocation
                | TokenKind::Type
                | TokenKind::Command
                | TokenKind::LBrace
                | TokenKind::RBrace => return,
                _ => self.pos += 1,
            }
        }
    }

    /// Require the `*` delimiter; if it's missing, report it and pretend it was there.
    fn expect_delim(&mut self, what: &str, line: usize) {
        if self.eat(TokenKind::Delim).is_none() {
            self.out.recoveries.push(Diagnostic {
                line,
                message: format!(
                    "Missing '*' delimiter after '{what}' (phrase-level recovery: auto-inserted)"
                ),
            });
        }
    }

    fn parse(mut self) -> ParseResult {
        while let Some(tok) = self.peek() {
            match tok.kind {
                TokenKind::LBrace => {
                    self.advance();
                    self.scope += 1;
                    self.out
                        .log
                        .push(format!("[PARSER] '{{' opens scope level {}", self.scope));
                }
                TokenKind::RBrace => {
                    self.advance();
                    // check_brackets already guarantees this never underflows.
                    self.scope = self.scope.saturating_sub(1);
                    self.out.log.push(format!(
                        "[PARSER] '}}' returns to scope level {}",
                        self.scope
                    ));
                }
                TokenKind::Invocation => self.parse_tap(),
                TokenKind::Type => self.parse_declaration(),
                TokenKind::Command => self.parse_command(),
                _ => {
                    self.error(
                        tok.line,
                        format!(
                            "Unexpected {} '{}' at the start of a statement",
                            tok.kind.category(),
                            tok.text
                        ),
                    );
                    self.advance();
                    self.synchronize();
                }
            }
        }
        self.out
    }

    fn parse_tap(&mut self) {
        let tap = self.advance().expect("caller checked");
        let line = tap.line;

        let source = match self.peek() {
            Some(t)
                if matches!(t.kind, TokenKind::Library | TokenKind::Guild)
                    || (t.kind == TokenKind::Id && t.text == "wastes") =>
            {
                self.advance();
                t.text.clone()
            }
            _ => {
                self.error(line, "Expected 'tap <land|guild>(<count>)'");
                self.synchronize();
                return;
            }
        };

        let count = if self.eat(TokenKind::LParen).is_some() {
            self.eat(TokenKind::Num).map(|n| n.text.clone())
        } else {
            None
        };
        let count = match count.and_then(|n| n.parse::<f64>().ok()) {
            Some(n) if n >= 0.0 && n <= u32::MAX as f64 => n as u32,
            _ => {
                self.error(
                    line,
                    format!("Expected 'tap {source}(<count>)' with a whole-number count"),
                );
                self.synchronize();
                return;
            }
        };
        if self.eat(TokenKind::RParen).is_none() {
            self.error(line, format!("Expected ')' after 'tap {source}({count}'"));
            self.synchronize();
            return;
        }
        self.eat(TokenKind::Delim); // optional for tap

        self.out.log.push(format!(
            "[PARSER] ✓ tap {source}({count}) matches: INVOCATION SOURCE LPAREN NUM RPAREN"
        ));
        self.out.statements.push(Stmt::Tap {
            source,
            count,
            line,
        });
    }

    fn parse_declaration(&mut self) {
        let ty_tok = self.advance().expect("caller checked");
        let line = ty_tok.line;
        let ty = VarType::from_keyword(&ty_tok.text).expect("lexer only emits valid types");

        // TYPE must be followed by an identifier.
        let name = match self.peek() {
            Some(t) if t.kind == TokenKind::Id => {
                self.advance();
                t.text.clone()
            }
            other => {
                let msg = match other {
                    None => format!("Type declaration incomplete - expected identifier after type '{}'", ty.name()),
                    Some(t) => match t.kind {
                        TokenKind::Num => format!(
                            "Identifier cannot start with a number '{}' - identifiers must start with a letter or underscore",
                            t.text
                        ),
                        TokenKind::Str => format!("Identifier cannot be a string literal - expected identifier, got {}", t.text),
                        TokenKind::Delim => format!("Expected identifier after type '{}', got delimiter '*'", ty.name()),
                        TokenKind::Assign => format!(
                            "Expected identifier after type '{}', got '->' operator - missing variable name",
                            ty.name()
                        ),
                        k if k.is_keyword() => {
                            format!("'{}' is a keyword and cannot be used as an identifier - choose a different name", t.text)
                        }
                        _ => format!("Expected identifier after type '{}', got '{}'", ty.name(), t.text),
                    },
                };
                self.error(line, msg);
                self.synchronize();
                return;
            }
        };

        let mut value = None;
        if self.eat(TokenKind::Assign).is_some() {
            match self.peek() {
                Some(t) if t.kind == TokenKind::Str => {
                    self.advance();
                    value = Some(Literal::Str(t.unquoted().to_string()));
                }
                Some(t) if t.kind == TokenKind::Num => {
                    self.advance();
                    value = Some(Literal::Num(t.text.clone()));
                }
                _ => {
                    self.error(
                        line,
                        format!("Missing value after '->' in declaration of '{name}'"),
                    );
                    self.synchronize();
                    return;
                }
            }
        }

        // A scroll (string) must be given a quoted string value.
        if ty == VarType::Scroll && !matches!(value, Some(Literal::Str(_))) {
            self.error(
                line,
                format!(
                    "scroll '{name}' requires a quoted string value: scroll {name} -> \"text\"*"
                ),
            );
            self.synchronize();
            return;
        }

        let actual = match &value {
            Some(v) => format!("TYPE ID ASSIGN {} *", v.kind_name()),
            None => "TYPE ID *".to_string(),
        };
        self.expect_delim(&name, line);
        let stmt = Stmt::Declare {
            ty,
            name,
            value,
            scope: self.scope,
            line,
        };
        self.out
            .log
            .push(format!("[PARSER] ✓ {} matches: {actual}", stmt.code()));
        self.out.statements.push(stmt);
    }

    fn parse_command(&mut self) {
        let cmd_tok = self.advance().expect("caller checked");
        let line = cmd_tok.line;
        let cmd = cmd_tok.text.clone();

        let arg = if let Some(id) = self.eat(TokenKind::Id) {
            // Short form: `bolt target*`
            Arg::Var(id.text.clone())
        } else if self.eat(TokenKind::LParen).is_some() {
            let arg = match self.peek() {
                Some(t) if t.kind == TokenKind::Id => Arg::Var(t.text.clone()),
                Some(t) if t.kind == TokenKind::Str => {
                    Arg::Lit(Literal::Str(t.unquoted().to_string()))
                }
                Some(t) if t.kind == TokenKind::Num => Arg::Lit(Literal::Num(t.text.clone())),
                _ => {
                    self.error(line, format!("Invalid or missing parameter for '{cmd}' - expected a string, number or variable"));
                    self.synchronize();
                    return;
                }
            };
            self.advance();
            if self.eat(TokenKind::RParen).is_none() {
                self.error(line, format!("Expected ')' after the parameter of '{cmd}'"));
                self.synchronize();
                return;
            }
            arg
        } else {
            self.error(line, format!("Expected a parameter after '{cmd}'"));
            self.synchronize();
            return;
        };

        let actual = format!("COMMAND LPAREN {} RPAREN *", arg.kind_name());
        let stmt = Stmt::Command {
            cmd,
            arg,
            scope: self.scope,
            line,
        };
        self.expect_delim(&stmt.code(), line);
        self.out
            .log
            .push(format!("[PARSER] ✓ {} matches: {actual}", stmt.code()));
        self.out.statements.push(stmt);
    }
}

/// Parse a token list. Bracket balance must be checked first with [`check_brackets`].
pub fn parse(tokens: &[Token]) -> ParseResult {
    Parser {
        tokens,
        pos: 0,
        scope: 0,
        out: ParseResult::default(),
    }
    .parse()
}
