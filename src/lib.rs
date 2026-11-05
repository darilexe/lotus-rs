//! Lotus: a programming language where every line of code costs mana.
//!
//! This crate is a Rust port of the original Python compiler. It runs the
//! same four stages:
//!
//! 1. [`lexer`]: lexical analysis
//! 2. [`parser`]: syntax analysis with error recovery
//! 3. [`semantic`] + [`runtime`]: type checking, the symbol table, and mana costs
//! 4. execution: resolving every statement and printing `bolt` output

pub mod lexer;
pub mod parser;
pub mod runtime;
pub mod semantic;

use parser::{Stmt, VarType};
use runtime::{build_pool, cost_of, pay, resolve};
use semantic::Symbol;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Lexer,
    Parser,
    Semantics,
    Execution,
}

impl Stage {
    pub fn name(self) -> &'static str {
        match self {
            Stage::Lexer => "LEXICAL ANALYSIS",
            Stage::Parser => "SYNTAX ANALYSIS",
            Stage::Semantics => "SEMANTIC ANALYSIS",
            Stage::Execution => "EXECUTION",
        }
    }
}

/// Everything a compile-and-run produced.
#[derive(Debug)]
pub struct Report {
    /// The full Grimoire Log, stage by stage.
    pub log: Vec<String>,
    /// Program output (what `bolt` printed).
    pub output: Vec<String>,
    /// Errors, each prefixed with its line number.
    pub errors: Vec<String>,
    pub symbols: Vec<Symbol>,
    /// `None` if everything succeeded, otherwise the stage that failed.
    pub failed: Option<Stage>,
}

impl Report {
    pub fn success(&self) -> bool {
        self.failed.is_none()
    }

    fn new() -> Self {
        Report {
            log: Vec::new(),
            output: Vec::new(),
            errors: Vec::new(),
            symbols: Vec::new(),
            failed: None,
        }
    }

    fn header(&mut self, n: usize, stage: Stage) {
        let bar = "=".repeat(50);
        if n > 1 {
            self.log.push(String::new());
        }
        self.log.push(bar.clone());
        self.log.push(format!("STAGE {n}: {}", stage.name()));
        self.log.push(bar);
    }

    fn fail(mut self, stage: Stage) -> Self {
        let bar = "=".repeat(50);
        self.log.push(String::new());
        self.log.push(bar.clone());
        self.log.push(match stage {
            Stage::Execution | Stage::Semantics => "✗ EXECUTION FAILED".to_string(),
            _ => "✗ COMPILATION FAILED".to_string(),
        });
        self.log.push(bar);
        self.failed = Some(stage);
        self
    }

    fn error(&mut self, line: usize, msg: impl AsRef<str>) {
        self.errors.push(format!("line {line}: {}", msg.as_ref()));
    }
}

/// Compile and run a Lotus program.
pub fn run(source: &str) -> Report {
    let mut r = Report::new();

    // ---- Stage 1: lexer ----
    r.header(1, Stage::Lexer);
    let tokens = match lexer::tokenize(source) {
        Ok(t) => t,
        Err(e) => {
            let line = match e {
                lexer::LexError::UnknownCharacter { line, .. }
                | lexer::LexError::UnterminatedString { line, .. } => line,
            };
            r.log
                .push(format!("[LEXER] ✗ SYNTAX ERROR (line {line}): {e}"));
            r.error(line, e.to_string());
            return r.fail(Stage::Lexer);
        }
    };
    for t in &tokens {
        r.log.push(format!(
            "[LEXER] Found '{}' -> Identified as {}",
            t.text,
            t.kind.category()
        ));
    }
    r.log.push(format!(
        "[LEXER] ✓ Tokenization complete: {} tokens found",
        tokens.len()
    ));

    // ---- Stage 2: parser ----
    r.header(2, Stage::Parser);
    r.log.push(
        "[PARSER] ⚡ CUSTOM DELIMITER RULE: * (asterisk) must terminate all statements".into(),
    );
    if let Err(d) = parser::check_brackets(&tokens) {
        r.log
            .push(format!("[PARSER] ✗ SYNTAX ERROR: {}", d.message));
        r.error(d.line, d.message);
        return r.fail(Stage::Parser);
    }
    let parsed = parser::parse(&tokens);
    r.log.extend(parsed.log.iter().cloned());
    for rec in &parsed.recoveries {
        r.log.push(format!(
            "[PARSER] ✗ SYNTAX ERROR (line {}): {}",
            rec.line, rec.message
        ));
        r.log
            .push("[RECOVERY] Phrase-level: auto-inserted missing '*' delimiter".into());
    }
    if !parsed.errors.is_empty() {
        for d in &parsed.errors {
            r.log.push(format!(
                "[PARSER] ✗ SYNTAX ERROR (line {}): {}",
                d.line, d.message
            ));
            r.error(d.line, &d.message);
        }
        return r.fail(Stage::Parser);
    }

    // ---- Stage 3: semantics (types, symbol table, mana) ----
    r.header(3, Stage::Semantics);
    let sem = semantic::analyze(&parsed.statements);
    r.log.extend(sem.log.iter().cloned());
    r.symbols = sem.symbols.entries().to_vec();
    if !sem.errors.is_empty() {
        for d in &sem.errors {
            r.error(d.line, &d.message);
        }
        return r.fail(Stage::Semantics);
    }

    let pool = build_pool(&parsed.statements);
    r.log.push(format!(
        "[SEMANTICS] Mana pool after tapping all lands: {pool}"
    ));
    // Pay colored costs first, then generic ones, the way a player would:
    // generic mana can come from any color, so it shouldn't use up the red
    // a later `bolt` needs.
    let mut check = pool;
    let mut order: Vec<&Stmt> = parsed.statements.iter().collect();
    order.sort_by_key(|s| matches!(cost_of(s, &sem.symbols), Some((None, _))));
    for stmt in order {
        let Some((color, cost)) = cost_of(stmt, &sem.symbols) else {
            continue;
        };
        let what = color.map(|c| c.name()).unwrap_or("generic");
        match pay(&mut check, stmt, &sem.symbols) {
            Ok(()) => r.log.push(format!(
                "[SEMANTICS] ✓ '{}' costs {cost} {what} mana: paid",
                stmt.code()
            )),
            Err(e) => {
                let msg = format!(
                    "INSUFFICIENT MANA: '{}' needs {} {what} mana, have {}",
                    stmt.code(),
                    e.need,
                    e.have
                );
                r.log.push(format!("[SEMANTICS] ✗ {msg}"));
                r.error(line_of(stmt), msg);
                return r.fail(Stage::Semantics);
            }
        }
    }

    // ---- Stage 4: execution ----
    r.header(4, Stage::Execution);
    for stmt in &parsed.statements {
        match stmt {
            Stmt::Tap { source, count, .. } => {
                r.log.push(format!("➜ Tapped {count} {source} for mana"));
            }
            Stmt::Declare { name, .. } => r.log.push(format!("✓ Bound '{name}' successfully")),
            Stmt::Command { cmd, arg, .. } => {
                let value = resolve(arg, &sem.symbols);
                if cmd == "bolt" {
                    r.log.push(format!("👁 OUTPUT (BOLT): {value}"));
                    r.output.push(value);
                } else {
                    r.log.push(format!(
                        "⚡ {} resolved silently on {value}",
                        cmd.to_uppercase()
                    ));
                }
            }
        }
    }
    let bar = "=".repeat(50);
    r.log.push(String::new());
    r.log.push(bar.clone());
    r.log.push("✓ EXECUTION SUCCESSFUL".into());
    r.log.push(bar);
    r
}

fn line_of(stmt: &Stmt) -> usize {
    match stmt {
        Stmt::Tap { line, .. } | Stmt::Declare { line, .. } | Stmt::Command { line, .. } => *line,
    }
}

/// The symbol table as a printable text table.
pub fn format_symbol_table(symbols: &[Symbol]) -> String {
    let mut rows = vec![[
        "Name".to_string(),
        "Type".into(),
        "Value".into(),
        "Level".into(),
        "Offset".into(),
        "Size".into(),
    ]];
    for s in symbols {
        let value = if s.ty == VarType::Scroll {
            format!("\"{}\"", s.value)
        } else {
            s.value.clone()
        };
        rows.push([
            s.name.clone(),
            s.ty.name().into(),
            value,
            s.level.to_string(),
            s.offset.to_string(),
            format!("{}B", s.size),
        ]);
    }
    let widths: Vec<usize> = (0..6)
        .map(|c| rows.iter().map(|r| r[c].chars().count()).max().unwrap_or(0))
        .collect();
    let mut out = String::new();
    for (i, row) in rows.iter().enumerate() {
        let cells: Vec<String> = row
            .iter()
            .zip(&widths)
            .map(|(cell, w)| format!("{cell:<w$}"))
            .collect();
        out.push_str(cells.join(" | ").trim_end());
        out.push('\n');
        if i == 0 {
            let sep: Vec<String> = widths.iter().map(|w| "-".repeat(*w)).collect();
            out.push_str(&sep.join("-+-"));
            out.push('\n');
        }
    }
    out
}
