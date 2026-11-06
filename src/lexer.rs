//! Stage 1: lexical analysis.
//!
//! Turns Lotus source text into a list of tokens. Keywords are recognised by
//! scanning a whole word first and then classifying it, which matches the
//! `\bword\b` rules of the original Python regex lexer.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    // keywords
    Invocation, // tap
    Guild,      // izzet, dimir, ...
    Library,    // mountain, island, swamp, plains, forest
    Type,       // mana, hybmana, scroll, status
    Command,    // bolt, stack, exile, seal, library
    // operators
    Assign,    // ->
    Equals,    // ==
    NotEquals, // !=
    Gte,       // >=
    Lte,       // <=
    Gt,        // >
    Lt,        // <
    // separators
    LBrace,
    RBrace,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Delim, // *
    Comma,
    // literals and names
    Str,
    Num,
    Id,
}

impl TokenKind {
    /// Compiler-textbook category used in the lexer log.
    pub fn category(self) -> &'static str {
        use TokenKind::*;
        match self {
            Invocation | Guild | Library | Type | Command => "keyword (reserved word)",
            Assign | Equals | NotEquals | Gte | Lte | Gt | Lt => "operator (symbol)",
            LBrace | RBrace | LParen | RParen | LBracket | RBracket | Delim => "separator",
            Comma => "comma",
            Str => "string literal",
            Num => "constant literal",
            Id => "identifier (name)",
        }
    }

    pub fn is_keyword(self) -> bool {
        use TokenKind::*;
        matches!(self, Invocation | Guild | Library | Type | Command)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    /// The exact source text (string literals keep their quotes).
    pub text: String,
    pub line: usize,
    pub col: usize,
}

impl Token {
    /// String literal contents without the surrounding quotes.
    pub fn unquoted(&self) -> &str {
        if self.kind == TokenKind::Str && self.text.len() >= 2 {
            &self.text[1..self.text.len() - 1]
        } else {
            &self.text
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum LexError {
    UnknownCharacter { ch: char, line: usize, col: usize },
    UnterminatedString { line: usize, col: usize },
}

impl fmt::Display for LexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LexError::UnknownCharacter { ch, col, .. } => {
                write!(f, "Unknown glyph '{ch}' at column {col}")
            }
            LexError::UnterminatedString { col, .. } => {
                write!(f, "Unterminated string starting at column {col}")
            }
        }
    }
}

pub const GUILDS: [&str; 10] = [
    "izzet", "dimir", "azorius", "simic", "rakdos", "boros", "golgari", "gruul", "orzhov",
    "selesnya",
];
pub const LIBRARIES: [&str; 5] = ["mountain", "island", "swamp", "plains", "forest"];
pub const TYPES: [&str; 4] = ["mana", "hybmana", "scroll", "status"];
pub const COMMANDS: [&str; 5] = ["bolt", "stack", "exile", "seal", "library"];

fn classify_word(word: &str) -> TokenKind {
    // Order matters: the Python lexer tried these groups in this order.
    if word == "tap" {
        TokenKind::Invocation
    } else if GUILDS.contains(&word) {
        TokenKind::Guild
    } else if LIBRARIES.contains(&word) {
        TokenKind::Library
    } else if TYPES.contains(&word) {
        TokenKind::Type
    } else if COMMANDS.contains(&word) {
        TokenKind::Command
    } else {
        TokenKind::Id
    }
}

struct Scanner<'a> {
    chars: Vec<char>,
    pos: usize,
    line: usize,
    col: usize,
    _src: &'a str,
}

impl<'a> Scanner<'a> {
    fn new(src: &'a str) -> Self {
        Scanner {
            chars: src.chars().collect(),
            pos: 0,
            line: 1,
            col: 1,
            _src: src,
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn peek_next(&self) -> Option<char> {
        self.chars.get(self.pos + 1).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += 1;
        if c == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(c)
    }
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Tokenize a whole program. Stops at the first character it doesn't recognise.
pub fn tokenize(src: &str) -> Result<Vec<Token>, LexError> {
    let mut s = Scanner::new(src);
    let mut tokens = Vec::new();

    while let Some(c) = s.peek() {
        let (line, col) = (s.line, s.col);

        if matches!(c, ' ' | '\t' | '\n' | '\r') {
            s.bump();
            continue;
        }

        let mut push = |kind: TokenKind, text: String| {
            tokens.push(Token {
                kind,
                text,
                line,
                col,
            });
        };

        if c.is_ascii_alphabetic() || c == '_' {
            let mut word = String::new();
            while let Some(c) = s.peek() {
                if !is_word_char(c) {
                    break;
                }
                word.push(c);
                s.bump();
            }
            push(classify_word(&word), word);
        } else if c.is_ascii_digit() {
            let mut num = String::new();
            while let Some(d) = s.peek().filter(char::is_ascii_digit) {
                num.push(d);
                s.bump();
            }
            // Optional fractional part: only if a digit follows the dot.
            if s.peek() == Some('.') && s.peek_next().is_some_and(|d| d.is_ascii_digit()) {
                num.push('.');
                s.bump();
                while let Some(d) = s.peek().filter(char::is_ascii_digit) {
                    num.push(d);
                    s.bump();
                }
            }
            push(TokenKind::Num, num);
        } else if c == '"' || c == '\'' {
            let quote = c;
            let mut text = String::from(quote);
            s.bump();
            loop {
                match s.bump() {
                    Some(ch) if ch == quote => {
                        text.push(ch);
                        break;
                    }
                    Some(ch) => text.push(ch),
                    None => return Err(LexError::UnterminatedString { line, col }),
                }
            }
            push(TokenKind::Str, text);
        } else {
            // Two-character operators first, then single characters.
            let two: Option<(TokenKind, &str)> = match (c, s.peek_next()) {
                ('-', Some('>')) => Some((TokenKind::Assign, "->")),
                ('=', Some('=')) => Some((TokenKind::Equals, "==")),
                ('!', Some('=')) => Some((TokenKind::NotEquals, "!=")),
                ('>', Some('=')) => Some((TokenKind::Gte, ">=")),
                ('<', Some('=')) => Some((TokenKind::Lte, "<=")),
                _ => None,
            };
            if let Some((kind, text)) = two {
                s.bump();
                s.bump();
                push(kind, text.to_string());
                continue;
            }
            let kind = match c {
                '{' => TokenKind::LBrace,
                '}' => TokenKind::RBrace,
                '(' => TokenKind::LParen,
                ')' => TokenKind::RParen,
                '[' => TokenKind::LBracket,
                ']' => TokenKind::RBracket,
                '*' => TokenKind::Delim,
                ',' => TokenKind::Comma,
                '>' => TokenKind::Gt,
                '<' => TokenKind::Lt,
                other => {
                    return Err(LexError::UnknownCharacter {
                        ch: other,
                        line,
                        col,
                    })
                }
            };
            s.bump();
            push(kind, c.to_string());
        }
    }

    Ok(tokens)
}
