use std::io::Read;
use std::process::ExitCode;

const USAGE: &str = "\
Usage: lotus [--quiet] <file.lotus>
       lotus [--quiet] -          (read the program from stdin)

Runs a Lotus program through all four compiler stages and prints the
Grimoire Log and the symbol table. With --quiet, prints only the program
output and any errors.";

fn main() -> ExitCode {
    let mut quiet = false;
    let mut path: Option<String> = None;
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "-q" | "--quiet" => quiet = true,
            "-h" | "--help" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            _ if path.is_none() => path = Some(arg),
            _ => {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            }
        }
    }

    let source = match path.as_deref() {
        None => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
        Some("-") => {
            let mut s = String::new();
            if let Err(e) = std::io::stdin().read_to_string(&mut s) {
                eprintln!("error: couldn't read stdin: {e}");
                return ExitCode::from(2);
            }
            s
        }
        Some(p) => match std::fs::read_to_string(p) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("error: couldn't read '{p}': {e}");
                return ExitCode::from(2);
            }
        },
    };

    let report = lotus::run(&source);

    if quiet {
        for line in &report.output {
            println!("{line}");
        }
    } else {
        for line in &report.log {
            println!("{line}");
        }
        if !report.symbols.is_empty() {
            println!("\n◆ SYMBOL TABLE");
            print!("{}", lotus::format_symbol_table(&report.symbols));
        }
    }

    if report.success() {
        ExitCode::SUCCESS
    } else {
        for e in &report.errors {
            eprintln!("error: {e}");
        }
        ExitCode::FAILURE
    }
}
