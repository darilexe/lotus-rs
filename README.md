# 🪷 Lotus, in Rust

A Rust port of the [Lotus compiler](https://github.com/darilexe/lotus-lang), the Magic: The Gathering–inspired language where every line of code costs mana.

The original is a Python/Tkinter app where you drag cards and tap lands by hand. This version is a **command-line compiler**: you give it a `.lotus` file, it taps every land for you, and it runs the same four stages, **lexer → parser → semantic analysis → execution**, printing the Grimoire Log and the symbol table.

> Built with AI assistance (Claude) as a way to learn Rust by rebuilding a project I already know inside out.

## Run it

Requires [Rust](https://rustup.rs) 1.75 or later. There are no dependencies.

```bash
cargo run -- examples/01-hello-world.lotus           # full compiler log + symbol table
cargo run -- --quiet examples/07-izzet-guild.lotus   # program output only
echo 'tap mountain(2) bolt("hi")*' | cargo run -- -  # read from stdin
```

The exit code is `0` on success and `1` if any stage fails, so it works in scripts.

```
$ cargo run -q -- examples/04-scoped-symbol-table.lotus
...
◆ SYMBOL TABLE
Name         | Type | Value | Level | Offset | Size
-------------+------+-------+-------+--------+-----
global       | mana | null  | 0     | 0      | 4B
local_target | mana | null  | 1     | 0      | 4B
offsetlocal  | mana | null  | 1     | 4      | 4B
```

## How it's organised

| File | Stage | What it does |
|---|---|---|
| `src/lexer.rs` | 1 | Hand-written scanner. Turns source text into tokens and reports unknown characters and unterminated strings with their line and column. |
| `src/parser.rs` | 2 | Checks bracket balance, then parses statements into an AST (`enum Stmt`). Missing `*` delimiters are auto-inserted (phrase-level recovery); other errors skip to the next statement (panic-mode recovery), so one mistake doesn't hide the rest. |
| `src/semantic.rs` | 3 | Type-checks declarations, warns on `null` values and redeclarations, and builds the scoped symbol table with 4-byte alignment for `mana` / `hybmana`. |
| `src/runtime.rs` | 3–4 | The mana system: lands and guilds, color-locked command costs, and paying for each statement. |
| `src/lib.rs` | all | Runs the pipeline and builds the report. |
| `src/main.rs` | | The command-line interface. |

## Tests

```bash
cargo test
```

There are 48 tests: unit tests for each stage, plus `tests/readme_cases.rs`, which runs the six test cases from the original README end to end:

| # | Example | Result |
|---|---|---|
| 1 | `01-hello-world.lotus` | ✅ prints `Enemy Commander` |
| 2 | `02-missing-delimiter.lotus` | ⚠️ syntax error, `*` auto-inserted, still runs |
| 3 | `03-type-mismatch.lotus` | ❌ `mana life_total -> "Twenty"` is a type mismatch |
| 4 | `04-scoped-symbol-table.lotus` | ✅ levels 0/1, aligned offsets 0, 0, 4 |
| 5 | `05-mana-leak.lotus` | ❌ `bolt` needs 2 red mana, have 1 |
| 6 | `06-color-mismatch.lotus` | ❌ `bolt` needs 2 red mana, have 0 (only blue tapped) |

`07-izzet-guild.lotus` adds a guild example: Izzet lands make blue and red mana at once.

## Differences from the Python version

Porting it surfaced a few spots where I made the behavior stricter or fixed a quirk:

- **Errors stop the program.** The Python version logged an error and skipped that line; here, any syntax or type error stops compilation before anything runs. A missing `*` is still only a warning, because the parser can fix it.
- **Costs are checked as a whole.** The program only runs if *every* statement can be paid for. The Python version checked each spell against the full pool without deducting, so two `bolt`s with 2 red mana passed the check.
- **Generic costs are paid last.** Declarations cost 1 mana of any color, so they're paid after colored costs, so they can't use up the blue a later `stack` needs.
- **Undeclared variables are always an error.** `bolt(name)` with an undeclared `name` used to print the word `name`; now it's a semantic error.
- **Overflow is caught.** A `mana` value that doesn't fit in 32 bits is a type error instead of silently becoming 0.
- **Better error locations.** Every error includes its line number, and lexer errors include the column.
