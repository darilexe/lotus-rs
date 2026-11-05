//! The six test cases from the original Lotus README, run end to end.

use lotus::{run, Stage};

fn example(name: &str) -> String {
    let path = format!("{}/examples/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

#[test]
fn case_1_declare_and_print() {
    let r = run(&example("01-hello-world.lotus"));
    assert!(r.success(), "{:?}", r.errors);
    assert_eq!(r.output, vec!["Enemy Commander"]);
}

#[test]
fn case_2_missing_delimiter_is_recovered() {
    let r = run(&example("02-missing-delimiter.lotus"));
    assert!(r.success(), "recovery lets the program still run");
    assert!(r.log.iter().any(|l| l.contains("[RECOVERY]")));
    assert_eq!(r.output, vec!["Enemy Commander"]);
}

#[test]
fn case_3_type_mismatch() {
    let r = run(&example("03-type-mismatch.lotus"));
    assert_eq!(r.failed, Some(Stage::Semantics));
    assert!(r.errors[0].contains("TYPE MISMATCH"));
    assert!(r.output.is_empty());
}

#[test]
fn case_4_scoped_symbol_table() {
    let r = run(&example("04-scoped-symbol-table.lotus"));
    assert!(r.success());
    let rows: Vec<(&str, usize, usize)> = r
        .symbols
        .iter()
        .map(|s| (s.name.as_str(), s.level, s.offset))
        .collect();
    assert_eq!(
        rows,
        vec![
            ("global", 0, 0),
            ("local_target", 1, 0),
            ("offsetlocal", 1, 4)
        ]
    );
}

#[test]
fn case_5_mana_leak() {
    let r = run(&example("05-mana-leak.lotus"));
    assert_eq!(r.failed, Some(Stage::Semantics));
    assert!(r.errors[0].contains("needs 2 RED mana, have 1"));
}

#[test]
fn case_6_color_mismatch() {
    let r = run(&example("06-color-mismatch.lotus"));
    assert_eq!(r.failed, Some(Stage::Semantics));
    assert!(r.errors[0].contains("needs 2 RED mana, have 0"));
}

#[test]
fn guild_lands_pay_for_two_colors() {
    let r = run(&example("07-izzet-guild.lotus"));
    assert!(r.success(), "{:?}", r.errors);
    assert_eq!(r.output, vec!["20", "Counterspell"]);
}

#[test]
fn unknown_glyph_stops_at_the_lexer() {
    let r = run("tap mountain(2)\nbolt(\"hi\")* @");
    assert_eq!(r.failed, Some(Stage::Lexer));
    assert!(r.errors[0].starts_with("line 2"));
}

#[test]
fn unbalanced_braces_stop_at_the_parser() {
    let r = run("tap mountain(2)\n{ mana x -> 1*");
    assert_eq!(r.failed, Some(Stage::Parser));
}

#[test]
fn every_statement_must_be_paid_for() {
    // 3 red covers one bolt (2) but not two (4): the program must not run at all.
    let r = run("tap mountain(3)\nbolt(1)*\nbolt(2)*");
    assert_eq!(r.failed, Some(Stage::Semantics));
    assert!(r.output.is_empty(), "nothing executes if any cost fails");
}

#[test]
fn empty_program_succeeds() {
    let r = run("");
    assert!(r.success());
    assert!(r.output.is_empty());
}

#[test]
fn generic_costs_dont_steal_colored_mana() {
    // 3 blue + 3 red. Paying the declaration with blue first would leave only
    // 2 blue for `stack` (cost 3), so generic costs are paid last.
    let r = run("tap izzet(3)\nmana a -> 1*\nstack(a)*");
    assert!(r.success(), "{:?}", r.errors);
}
