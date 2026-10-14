//! The `tds` command-line tool.

use std::io::IsTerminal;
use std::process::ExitCode;

const USAGE: &str = "\
tds: the TDS language tool (Phase 1 prototype)

Usage:
    tds run <file.tds> [args...]   check and run a program
    tds check <file.tds>           check a program without running it
    tds parse <file.tds>           print the syntax tree (for debugging)
    tds tokens <file.tds>          print the tokens (for debugging)
    tds help                       show this message
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(command) = args.first() else {
        eprint!("{USAGE}");
        return ExitCode::FAILURE;
    };
    match command.as_str() {
        "run" | "check" | "parse" | "tokens" => {
            let Some(path) = args.get(1) else {
                eprintln!("error: `tds {command}` needs a file name\n");
                eprint!("{USAGE}");
                return ExitCode::FAILURE;
            };
            let src = match std::fs::read_to_string(path) {
                Ok(src) => src,
                Err(e) => {
                    eprintln!("error: can't read `{path}`: {e}");
                    return ExitCode::FAILURE;
                }
            };
            match command.as_str() {
                "run" => run(path, &src, args[2..].to_vec()),
                "check" => check(path, &src),
                "parse" => parse(path, &src),
                _ => tokens(path, &src),
            }
        }
        "help" | "--help" | "-h" => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        other => {
            eprintln!("error: unknown command `{other}`\n");
            eprint!("{USAGE}");
            ExitCode::FAILURE
        }
    }
}

fn report(diagnostics: &[tds_diagnostics::Diagnostic], path: &str, src: &str) {
    let color = std::io::stderr().is_terminal();
    eprint!("{}", tds_cli::render(diagnostics, path, src, color));
}

fn run(path: &str, src: &str, args: Vec<String>) -> ExitCode {
    let result = tds_cli::with_big_stack(|| {
        let mut stdout = std::io::stdout().lock();
        tds_cli::run(src, &mut stdout, args)
    });
    match result {
        Ok(warnings) => {
            report(&warnings, path, src);
            ExitCode::SUCCESS
        }
        Err(errors) => {
            report(&errors, path, src);
            ExitCode::FAILURE
        }
    }
}

fn check(path: &str, src: &str) -> ExitCode {
    match tds_cli::check(src) {
        Ok((_, warnings)) => {
            report(&warnings, path, src);
            ExitCode::SUCCESS
        }
        Err(errors) => {
            report(&errors, path, src);
            let count = errors
                .iter()
                .filter(|d| d.severity == tds_diagnostics::Severity::Error)
                .count();
            eprintln!("{count} error{} found", if count == 1 { "" } else { "s" });
            ExitCode::FAILURE
        }
    }
}

fn parse(path: &str, src: &str) -> ExitCode {
    let (module, errors) = tds_parser::parse_module(src);
    print!("{}", tds_ast::dump_module(&module));
    report(&errors, path, src);
    if errors.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn tokens(path: &str, src: &str) -> ExitCode {
    let out = tds_lexer::lex(src);
    for token in &out.tokens {
        println!(
            "{:>5}..{:<5} {}",
            token.span.start, token.span.end, token.kind
        );
    }
    report(&out.errors, path, src);
    if out.errors.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
