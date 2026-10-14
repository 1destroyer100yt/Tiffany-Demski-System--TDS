//! The `tds` command-line tool.

use std::io::IsTerminal;
use std::process::ExitCode;

const USAGE: &str = "\
tds: the TDS language tool (Phase 1 prototype)

Usage:
    tds run <file.tds> [args...]   check and run a program
    tds check <file.tds>           check a program without running it
    tds repl                       start an interactive prompt
    tds parse <file.tds>           print the syntax tree (for debugging)
    tds tokens <file.tds>          print the tokens (for debugging)
    tds explain <code>             explain an error code, like TDS0412
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
                "run" => run(path, args[2..].to_vec()),
                "check" => check(path),
                "parse" => parse(path, &src),
                _ => tokens(path, &src),
            }
        }
        "repl" => {
            let color = std::io::stderr().is_terminal();
            tds_cli::with_big_stack(|| {
                let mut stdin = std::io::stdin().lock();
                tds_cli::repl::run(&mut stdin, color);
            });
            ExitCode::SUCCESS
        }
        "explain" => {
            let Some(code) = args.get(1) else {
                eprintln!("error: `tds explain` needs an error code, like `tds explain TDS0412`");
                return ExitCode::FAILURE;
            };
            match tds_diagnostics::explain(code) {
                Some(text) => {
                    println!(
                        "{}

{text}",
                        code.to_uppercase()
                    );
                    ExitCode::SUCCESS
                }
                None => {
                    eprintln!("error: `{code}` isn't a TDS error code");
                    ExitCode::FAILURE
                }
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
    report_with(diagnostics, &tds_diagnostics::SourceMap::single(path, src));
}

fn report_with(diagnostics: &[tds_diagnostics::Diagnostic], sources: &tds_diagnostics::SourceMap) {
    let color = std::io::stderr().is_terminal();
    eprint!("{}", tds_cli::render_with(diagnostics, sources, color));
}

/// Prints the errors and a one-line summary with a pointer to `tds explain`.
fn report_errors(
    diagnostics: &[tds_diagnostics::Diagnostic],
    sources: &tds_diagnostics::SourceMap,
) {
    report_with(diagnostics, sources);
    let errors: Vec<&str> = diagnostics
        .iter()
        .filter(|d| d.severity == tds_diagnostics::Severity::Error)
        .map(|d| d.code)
        .collect();
    if let Some(first) = errors.first() {
        let count = errors.len();
        eprintln!(
            "{count} error{} found. For more about an error, run `tds explain {first}`.",
            if count == 1 { "" } else { "s" }
        );
    }
}

/// Loads and checks a program, reporting warnings or errors.
fn compile(path: &str) -> Option<tds_cli::Checked> {
    match tds_cli::compile_file(std::path::Path::new(path)) {
        Ok(checked) => {
            report_with(&checked.warnings, &checked.sources);
            Some(checked)
        }
        Err(failure) => {
            report_errors(&failure.diagnostics, &failure.sources);
            None
        }
    }
}

fn run(path: &str, args: Vec<String>) -> ExitCode {
    let Some(checked) = compile(path) else {
        return ExitCode::FAILURE;
    };
    let result = tds_cli::with_big_stack(|| {
        let mut stdout = std::io::stdout().lock();
        tds_cli::interpret(&checked, &mut stdout, args)
    });
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(panic) => {
            report_errors(&[panic], &checked.sources);
            ExitCode::FAILURE
        }
    }
}

fn check(path: &str) -> ExitCode {
    match compile(path) {
        Some(_) => ExitCode::SUCCESS,
        None => ExitCode::FAILURE,
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
