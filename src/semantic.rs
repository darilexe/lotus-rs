//! Stage 3 (part 1): semantic analysis.
//!
//! Type-checks every declaration, builds the scoped symbol table with
//! byte-aligned memory offsets, and checks that commands only use variables
//! that were declared.

use std::collections::{BTreeMap, HashMap};

use crate::parser::{Arg, Diagnostic, Literal, Stmt, VarType};

#[derive(Debug, Clone, PartialEq)]
pub struct Symbol {
    pub name: String,
    pub ty: VarType,
    /// The stored value as text; `"null"` when declared without one.
    pub value: String,
    pub level: usize,
    pub offset: usize,
    pub bytes: Vec<u8>,
    pub size: usize,
}

/// Symbol table: one entry per name, plus a running offset for each scope level.
#[derive(Debug, Default)]
pub struct SymbolTable {
    entries: Vec<Symbol>,
    index: HashMap<String, usize>,
    level_offsets: BTreeMap<usize, usize>,
}

impl SymbolTable {
    pub fn get(&self, name: &str) -> Option<&Symbol> {
        self.index.get(name).map(|&i| &self.entries[i])
    }

    pub fn entries(&self) -> &[Symbol] {
        &self.entries
    }

    /// Insert or redeclare a symbol. Numeric types are padded to a 4-byte boundary.
    fn bind(&mut self, name: &str, ty: VarType, value: String, level: usize) -> &Symbol {
        let (bytes, size) = encode(ty, &value);
        let current = *self.level_offsets.entry(level).or_insert(0);
        let (offset, _padding) = aligned_offset(ty, current, size);
        self.level_offsets.insert(level, offset + size);

        let sym = Symbol {
            name: name.to_string(),
            ty,
            value,
            level,
            offset,
            bytes,
            size,
        };
        let i = match self.index.get(name) {
            Some(&i) => {
                self.entries[i] = sym;
                i
            }
            None => {
                self.entries.push(sym);
                self.index.insert(name.to_string(), self.entries.len() - 1);
                self.entries.len() - 1
            }
        };
        &self.entries[i]
    }
}

/// Byte representation and size of a value.
///
/// * `mana`    : 4 bytes, 32-bit signed integer (big-endian)
/// * `hybmana` : 4 bytes, 32-bit float (big-endian)
/// * `scroll`  : the UTF-8 bytes, size = length + 1 for the null terminator
/// * `status`  : 1 byte
pub fn encode(ty: VarType, value: &str) -> (Vec<u8>, usize) {
    match ty {
        VarType::Mana => {
            let n = if value == "null" {
                0
            } else {
                value.parse::<f64>().map(|f| f as i32).unwrap_or(0)
            };
            (n.to_be_bytes().to_vec(), 4)
        }
        VarType::HybMana => {
            let f = if value == "null" {
                0.0
            } else {
                value.parse::<f32>().unwrap_or(0.0)
            };
            let bytes = if value == "null" {
                vec![0; 4]
            } else {
                f.to_be_bytes().to_vec()
            };
            (bytes, 4)
        }
        VarType::Scroll => {
            let bytes = value.as_bytes().to_vec();
            let size = bytes.len() + 1;
            (bytes, size)
        }
        VarType::Status => (value.as_bytes().iter().take(1).copied().collect(), 1),
    }
}

/// Where a value of `ty` can start, given the next free offset.
/// Returns `(aligned_offset, padding_bytes)`.
pub fn aligned_offset(ty: VarType, current: usize, size: usize) -> (usize, usize) {
    if size == 0 || !ty.is_numeric() {
        return (current, 0);
    }
    let padding = (4 - current % 4) % 4;
    (current + padding, padding)
}

fn hex(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        "00".into()
    } else {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }
}

#[derive(Debug, Default)]
pub struct SemanticResult {
    pub symbols: SymbolTable,
    pub errors: Vec<Diagnostic>,
    pub warnings: Vec<Diagnostic>,
    pub log: Vec<String>,
}

/// Type-check a declaration's value. Returns an error message on mismatch.
fn check_value(ty: VarType, name: &str, value: &Option<Literal>) -> Result<(), String> {
    let Some(v) = value else { return Ok(()) };
    if ty.is_numeric() {
        let parsed = v.text().parse::<f64>();
        match parsed {
            Err(_) => {
                return Err(format!(
                    "TYPE MISMATCH: {} '{name}' requires a NUMERIC value, got STRING \"{}\"",
                    ty.name(),
                    v.text()
                ))
            }
            Ok(n) if ty == VarType::Mana && (n < i32::MIN as f64 || n > i32::MAX as f64) => {
                return Err(format!(
                    "mana '{name}' value {} doesn't fit in 4 bytes (32-bit integer)",
                    v.text()
                ));
            }
            Ok(_) => {}
        }
    }
    Ok(())
}

pub fn analyze(statements: &[Stmt]) -> SemanticResult {
    let mut r = SemanticResult::default();

    for stmt in statements {
        match stmt {
            Stmt::Tap { .. } => {}
            Stmt::Declare {
                ty,
                name,
                value,
                scope,
                line,
            } => {
                r.log.push(format!(
                    "[SEMANTICS] Checking type compatibility for '{name}'..."
                ));
                if let Err(msg) = check_value(*ty, name, value) {
                    r.log.push(format!("[SEMANTICS] ✗ {msg}"));
                    r.errors.push(Diagnostic {
                        line: *line,
                        message: msg,
                    });
                    continue;
                }
                match value {
                    None => {
                        let msg = format!("{} '{name}' initialized to null (no value)", ty.name());
                        r.log.push(format!("[SEMANTICS] ⚠ Warning: {msg}"));
                        r.warnings.push(Diagnostic {
                            line: *line,
                            message: msg,
                        });
                    }
                    Some(v) => r.log.push(format!(
                        "[SEMANTICS] ✓ Value '{}' is compatible with {}",
                        v.text(),
                        ty.name()
                    )),
                }
                if let Some(prev) = r.symbols.get(name) {
                    let msg = format!("Variable '{name}' already declared at level {}", prev.level);
                    r.log.push(format!("[SEMANTICS] ⚠ Warning: {msg}"));
                    r.warnings.push(Diagnostic {
                        line: *line,
                        message: msg,
                    });
                }
                let text = value
                    .as_ref()
                    .map(|v| v.text().to_string())
                    .unwrap_or_else(|| "null".into());
                let sym = r.symbols.bind(name, *ty, text, *scope);
                r.log.push(format!(
                    "[SEMANTICS] Bound '{name}': level {}, offset {}, {} bytes (0x{})",
                    sym.level,
                    sym.offset,
                    sym.size,
                    hex(&sym.bytes)
                ));
            }
            Stmt::Command { cmd, arg, line, .. } => {
                if let Arg::Var(v) = arg {
                    if r.symbols.get(v).is_none() {
                        let msg = format!("Variable '{v}' used by '{cmd}' was never declared");
                        r.log.push(format!("[SEMANTICS] ✗ {msg}"));
                        r.errors.push(Diagnostic {
                            line: *line,
                            message: msg,
                        });
                        continue;
                    }
                }
                r.log
                    .push(format!("[SEMANTICS] ✓ '{}' is well-formed", stmt.code()));
            }
        }
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{lexer::tokenize, parser::parse};

    fn analyze_src(src: &str) -> SemanticResult {
        analyze(&parse(&tokenize(src).unwrap()).statements)
    }

    #[test]
    fn type_mismatch_is_an_error() {
        let r = analyze_src("mana life_total -> \"Twenty\"*");
        assert_eq!(r.errors.len(), 1);
        assert!(r.errors[0].message.contains("TYPE MISMATCH"));
        assert!(
            r.symbols.get("life_total").is_none(),
            "bad declarations aren't bound"
        );
    }

    #[test]
    fn numeric_string_is_accepted_for_mana() {
        // Same as the Python version: "5" parses as a number.
        assert!(analyze_src("mana x -> \"5\"*").errors.is_empty());
    }

    #[test]
    fn mana_overflow() {
        assert!(!analyze_src("mana big -> 99999999999*").errors.is_empty());
    }

    #[test]
    fn scoped_offsets_match_python_test_case_4() {
        // README test 4: levels 0/1 and aligned offsets 0, 0, 4.
        let r = analyze_src("mana global*\n{\n mana local_target*\n mana offsetlocal*\n}");
        let got: Vec<(&str, usize, usize)> = r
            .symbols
            .entries()
            .iter()
            .map(|s| (s.name.as_str(), s.level, s.offset))
            .collect();
        assert_eq!(
            got,
            vec![
                ("global", 0, 0),
                ("local_target", 1, 0),
                ("offsetlocal", 1, 4)
            ]
        );
        assert_eq!(r.warnings.len(), 3, "three null initialisations");
    }

    #[test]
    fn alignment_pads_after_strings() {
        // "hi" takes 3 bytes (0..3), so the next mana is padded to offset 4.
        let r =
            analyze_src("scroll s -> \"hi\"*\nmana n -> 1*\nstatus f -> 't'*\nhybmana h -> 1.5*");
        let offsets: Vec<usize> = r.symbols.entries().iter().map(|s| s.offset).collect();
        assert_eq!(offsets, vec![0, 4, 8, 12]);
    }

    #[test]
    fn encodings() {
        assert_eq!(encode(VarType::Mana, "20"), (vec![0, 0, 0, 20], 4));
        assert_eq!(encode(VarType::Mana, "null"), (vec![0, 0, 0, 0], 4));
        assert_eq!(
            encode(VarType::HybMana, "1.5"),
            (1.5f32.to_be_bytes().to_vec(), 4)
        );
        assert_eq!(encode(VarType::Scroll, "abc"), (b"abc".to_vec(), 4));
        assert_eq!(encode(VarType::Status, "true"), (b"t".to_vec(), 1));
    }

    #[test]
    fn undeclared_variable() {
        let r = analyze_src("bolt(ghost)*");
        assert!(r.errors[0].message.contains("never declared"));
    }

    #[test]
    fn redeclaration_warns() {
        let r = analyze_src("mana x -> 1*\nmana x -> 2*");
        assert!(r.errors.is_empty());
        assert!(r
            .warnings
            .iter()
            .any(|w| w.message.contains("already declared")));
        assert_eq!(r.symbols.get("x").unwrap().value, "2");
    }
}
