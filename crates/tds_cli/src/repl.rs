//! `tds repl`: an interactive prompt.
//!
//! Each entry (a line, or a block of lines) is parsed, checked and run in
//! one persistent session: declarations and top-level variables stay
//! around for later entries. An expression's value is printed, like in
//! Python.
//!
//! Entries are atomic. If one has a compile error, nothing in it runs. If
//! it panics partway through, the session goes back to how it was before
//! the entry (output already printed stays printed).

use std::io::{BufRead, Write};

use tds_ast::{Stmt, TopLevel};
use tds_check::ReplChecker;
use tds_diagnostics::{codes, Diagnostic, Severity};
use tds_interp::Interpreter;

/// A REPL session.
pub struct Session<'out> {
    checker: ReplChecker,
    interp: Interpreter<'out>,
}

/// What happened to one entry.
#[derive(Debug, Default)]
pub struct Outcome {
    /// Errors, warnings or a runtime panic, to render against the entry.
    pub diagnostics: Vec<Diagnostic>,
}

impl<'out> Session<'out> {
    /// Program output (from `print` and expression values) goes to `out`.
    pub fn new(out: &'out mut dyn Write) -> Session<'out> {
        Session {
            checker: ReplChecker::new(),
            interp: Interpreter::new(out, Vec::new()),
        }
    }

    /// Parses, checks and runs one entry.
    pub fn eval(&mut self, src: &str) -> Outcome {
        let (entries, parse_errors) = tds_parser::parse_repl(src);
        if !parse_errors.is_empty() {
            return Outcome {
                diagnostics: parse_errors,
            };
        }

        let saved_checker = self.checker.clone();
        let mut diagnostics = self.checker.check(&entries);
        if diagnostics.iter().any(|d| d.severity == Severity::Error) {
            return Outcome { diagnostics };
        }

        let saved_interp = self.interp.save_state();
        let items = entries.iter().filter_map(|e| match e {
            TopLevel::Item(item) => Some(item),
            TopLevel::Stmt(_) => None,
        });
        self.interp.load_items(items);
        let stmts: Vec<&Stmt> = entries
            .iter()
            .filter_map(|e| match e {
                TopLevel::Stmt(stmt) => Some(stmt),
                TopLevel::Item(_) => None,
            })
            .collect();
        for stmt in stmts {
            match self.interp.run_stmt(stmt) {
                Ok(Some(value)) => self.interp.write_line(&value.repr()),
                Ok(None) => {}
                Err(panic) => {
                    self.checker = saved_checker;
                    self.interp.restore_state(saved_interp);
                    diagnostics.push(panic);
                    break;
                }
            }
        }
        Outcome { diagnostics }
    }

    /// Forgets every declaration and variable.
    pub fn reset(&mut self) {
        self.checker = ReplChecker::new();
        self.interp.reset();
    }

    /// Flushes program output.
    pub fn flush(&mut self) {
        self.interp.flush();
    }
}

/// Whether the text typed so far is an unfinished entry, so the prompt
/// should ask for another line.
///
/// An entry continues while brackets are open and after a line ending in
/// an operator or `,`. A line ending in `:` starts a block, and a block
/// continues until an empty line.
pub fn needs_more_input(buffer: &str) -> bool {
    use tds_lexer::TokenKind;

    let lexed = tds_lexer::lex(buffer);
    if lexed
        .errors
        .iter()
        .any(|e| e.code == codes::UNCLOSED_BRACKET)
    {
        return true;
    }
    let in_block = lexed.tokens.iter().any(|t| t.kind == TokenKind::Indent);
    if in_block {
        return !buffer.lines().last().is_some_and(|l| l.trim().is_empty());
    }
    let last_token = lexed.tokens.iter().rev().find(|t| {
        !matches!(
            t.kind,
            TokenKind::Newline | TokenKind::Dedent | TokenKind::Eof
        )
    });
    last_token.is_some_and(|t| t.kind == TokenKind::Colon || t.kind.continues_line())
}

const HELP: &str = "\
Type TDS code and press Enter. Declarations (fn, struct, enum) and
statements both work here. An expression's value is printed.

A line ending in `:` starts a block: keep typing indented lines and finish
with an empty line.

Commands:
    :help    show this message
    :reset   forget everything declared so far
    :quit    leave (or press Ctrl-D)
";

/// Runs the interactive loop: reads entries from `input`, prints prompts
/// and results to stdout and diagnostics to stderr.
pub fn run(input: &mut dyn BufRead, color: bool) {
    let mut prompts = std::io::stdout();
    let mut errors = std::io::stderr();
    let mut program_out = std::io::stdout();
    let mut session = Session::new(&mut program_out);
    let _ = writeln!(
        prompts,
        "TDS {} REPL. Type :help for help, :quit to leave.",
        env!("CARGO_PKG_VERSION")
    );

    let mut buffer = String::new();
    loop {
        let prompt = if buffer.is_empty() { ">>> " } else { "... " };
        let _ = write!(prompts, "{prompt}");
        let _ = prompts.flush();

        let mut line = String::new();
        match input.read_line(&mut line) {
            Ok(0) | Err(_) => {
                let _ = writeln!(prompts);
                break;
            }
            Ok(_) => {}
        }
        let line = line.trim_end_matches(['\n', '\r']);

        if buffer.is_empty() {
            match line.trim() {
                "" => continue,
                ":quit" | ":q" | ":exit" => break,
                ":help" | ":h" => {
                    let _ = write!(prompts, "{HELP}");
                    continue;
                }
                ":reset" => {
                    session.reset();
                    let _ = writeln!(prompts, "session reset");
                    continue;
                }
                cmd if cmd.starts_with(':') => {
                    let _ = writeln!(errors, "unknown command `{cmd}`; type :help for help");
                    continue;
                }
                _ => {}
            }
        }

        buffer.push_str(line);
        buffer.push('\n');
        if needs_more_input(&buffer) {
            continue;
        }

        let outcome = session.eval(&buffer);
        session.flush();
        let rendered = crate::render(&outcome.diagnostics, "<repl>", &buffer, color);
        let _ = errors.write_all(rendered.as_bytes());
        let _ = errors.flush();
        buffer.clear();
    }
}
