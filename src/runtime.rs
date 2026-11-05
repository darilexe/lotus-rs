//! Stage 3 (part 2) and stage 4: the mana system and execution.
//!
//! Every `tap` puts lands into play, and the CLI taps all of them for mana.
//! Each statement then has to be paid for, in order:
//!
//! * declarations are colorless and cost 1 mana of any color
//! * commands need mana of their own color (`bolt` needs red, and so on)
//!
//! The cost check runs on a copy of the mana pool first, so the program only
//! executes if every statement can be paid for.

use std::fmt;

use crate::parser::{Arg, Literal, Stmt};
use crate::semantic::SymbolTable;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    White,
    Blue,
    Black,
    Red,
    Green,
    /// Produced by `wastes`; only usable for colorless costs.
    Wubrg,
}

impl Color {
    pub const ALL: [Color; 6] = [
        Color::White,
        Color::Blue,
        Color::Black,
        Color::Red,
        Color::Green,
        Color::Wubrg,
    ];

    fn idx(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            Color::White => "WHITE",
            Color::Blue => "BLUE",
            Color::Black => "BLACK",
            Color::Red => "RED",
            Color::Green => "GREEN",
            Color::Wubrg => "WUBRG",
        }
    }
}

/// The colors of mana a land or guild produces.
pub fn land_colors(source: &str) -> Option<&'static [Color]> {
    use Color::*;
    Some(match source {
        "mountain" => &[Red],
        "island" => &[Blue],
        "swamp" => &[Black],
        "plains" => &[White],
        "forest" => &[Green],
        "wastes" => &[Wubrg],
        "izzet" => &[Blue, Red],
        "dimir" => &[Blue, Black],
        "azorius" => &[Blue, White],
        "simic" => &[Blue, Green],
        "rakdos" => &[Black, Red],
        "boros" => &[Red, White],
        "golgari" => &[Black, Green],
        "gruul" => &[Red, Green],
        "orzhov" => &[White, Black],
        "selesnya" => &[White, Green],
        _ => return None,
    })
}

/// Color and base cost of each command.
pub fn command_cost(cmd: &str) -> Option<(Color, u32)> {
    Some(match cmd {
        "bolt" => (Color::Red, 2),
        "stack" => (Color::Blue, 3),
        "exile" => (Color::Black, 2),
        "seal" => (Color::White, 4),
        "library" => (Color::Green, 0), // dynamic: size of the argument
        _ => return None,
    })
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ManaPool([u32; 6]);

impl ManaPool {
    pub fn get(&self, c: Color) -> u32 {
        self.0[c.idx()]
    }

    pub fn add(&mut self, c: Color, n: u32) {
        self.0[c.idx()] = self.0[c.idx()].saturating_add(n);
    }

    pub fn total(&self) -> u32 {
        self.0.iter().sum()
    }

    /// Pay `n` of a specific color.
    fn pay_colored(&mut self, c: Color, n: u32) -> Result<(), ManaError> {
        let have = self.get(c);
        if have < n {
            return Err(ManaError {
                need: n,
                have,
                color: Some(c),
            });
        }
        self.0[c.idx()] -= n;
        Ok(())
    }

    /// Pay `n` generic mana. WUBRG goes first because it can't pay colored
    /// costs anyway, then W, U, B, R, G.
    fn pay_generic(&mut self, n: u32) -> Result<(), ManaError> {
        let have = self.total();
        if have < n {
            return Err(ManaError {
                need: n,
                have,
                color: None,
            });
        }
        let mut left = n;
        use Color::*;
        for c in [Wubrg, White, Blue, Black, Red, Green] {
            let take = left.min(self.get(c));
            self.0[c.idx()] -= take;
            left -= take;
        }
        Ok(())
    }
}

impl fmt::Display for ManaPool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let parts: Vec<String> = Color::ALL
            .iter()
            .filter(|c| self.get(**c) > 0)
            .map(|c| format!("{} {}", self.get(*c), c.name()))
            .collect();
        if parts.is_empty() {
            write!(f, "empty")
        } else {
            write!(f, "{}", parts.join(", "))
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ManaError {
    pub need: u32,
    pub have: u32,
    /// `None` for a colorless (generic) cost.
    pub color: Option<Color>,
}

/// Tap every land the program put into play.
pub fn build_pool(statements: &[Stmt]) -> ManaPool {
    let mut pool = ManaPool::default();
    for s in statements {
        if let Stmt::Tap { source, count, .. } = s {
            for &c in land_colors(source).unwrap_or(&[]) {
                pool.add(c, *count);
            }
        }
    }
    pool
}

/// What a statement costs: `(Some(color), n)` for commands, `(None, 1)` for declarations.
pub fn cost_of(stmt: &Stmt, symbols: &SymbolTable) -> Option<(Option<Color>, u32)> {
    match stmt {
        Stmt::Tap { .. } => None,
        Stmt::Declare { .. } => Some((None, 1)),
        Stmt::Command { cmd, arg, .. } => {
            let (color, base) = command_cost(cmd)?;
            let cost = if cmd == "library" {
                // library[] costs the size of what it searches.
                resolve(arg, symbols).chars().count() as u32
            } else {
                base
            };
            Some((Some(color), cost))
        }
    }
}

/// Pay for one statement out of `pool`.
pub fn pay(pool: &mut ManaPool, stmt: &Stmt, symbols: &SymbolTable) -> Result<(), ManaError> {
    match cost_of(stmt, symbols) {
        None => Ok(()),
        Some((Some(c), n)) => pool.pay_colored(c, n),
        Some((None, n)) => pool.pay_generic(n),
    }
}

/// The value a command argument refers to.
pub fn resolve(arg: &Arg, symbols: &SymbolTable) -> String {
    match arg {
        Arg::Var(name) => symbols
            .get(name)
            .map(|s| s.value.clone())
            .unwrap_or_else(|| name.clone()),
        Arg::Lit(Literal::Str(s)) | Arg::Lit(Literal::Num(s)) => s.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{lexer::tokenize, parser::parse, semantic::analyze};

    fn stmts(src: &str) -> Vec<Stmt> {
        parse(&tokenize(src).unwrap()).statements
    }

    #[test]
    fn taps_fill_the_pool() {
        let pool = build_pool(&stmts("tap mountain(3)\ntap izzet(2)\ntap wastes(1)"));
        assert_eq!(pool.get(Color::Red), 5);
        assert_eq!(pool.get(Color::Blue), 2);
        assert_eq!(pool.get(Color::Wubrg), 1);
        assert_eq!(pool.total(), 8);
    }

    #[test]
    fn colored_cost_needs_the_right_color() {
        let s = stmts("tap island(5)\nbolt(\"hi\")*");
        let syms = analyze(&s).symbols;
        let mut pool = build_pool(&s);
        let err = pay(&mut pool, &s[1], &syms).unwrap_err();
        assert_eq!(
            err,
            ManaError {
                need: 2,
                have: 0,
                color: Some(Color::Red)
            }
        );
    }

    #[test]
    fn generic_cost_uses_any_color() {
        let s = stmts("tap wastes(1)\nmana x -> 1*");
        let syms = analyze(&s).symbols;
        let mut pool = build_pool(&s);
        assert!(pay(&mut pool, &s[1], &syms).is_ok());
        assert_eq!(pool.total(), 0);
    }

    #[test]
    fn wastes_cannot_pay_colored_costs() {
        let s = stmts("tap wastes(5)\nbolt(1)*");
        let syms = analyze(&s).symbols;
        let mut pool = build_pool(&s);
        assert!(pay(&mut pool, &s[1], &syms).is_err());
    }

    #[test]
    fn library_costs_argument_size() {
        let s = stmts("tap forest(5)\nscroll deck -> \"abcd\"*\nlibrary(deck)*");
        let syms = analyze(&s).symbols;
        assert_eq!(cost_of(&s[2], &syms), Some((Some(Color::Green), 4)));
    }

    #[test]
    fn pool_display() {
        let pool = build_pool(&stmts("tap boros(1)"));
        assert_eq!(pool.to_string(), "1 WHITE, 1 RED");
        assert_eq!(ManaPool::default().to_string(), "empty");
    }
}
