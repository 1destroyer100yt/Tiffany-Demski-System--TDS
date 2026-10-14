//! The compiler driver: runs the pipeline stages in order and renders
//! diagnostics. The `tds` binary (`main.rs`) and the test suites both go
//! through these functions, so they always see the same behavior.
//!
//! Phase 1 pipeline:
//!
//! ```text
//! source → lexer → parser → type/ownership checker → interpreter
//! ```

use std::io::Write;

use codespan_reporting::diagnostic::{
    Diagnostic as CsDiagnostic, Label as CsLabel, Severity as CsSeverity,
};
use codespan_reporting::files::SimpleFile;
use codespan_reporting::term::{self, termcolor, Config, Styles, StylesWriter};
use tds_ast::Module;
use tds_diagnostics::{codes, Diagnostic, Severity, Span};
use tds_interp::Interpreter;

/// Parses and checks a source file. On success returns the module, ready
/// to run, plus any warnings.
pub fn check(src: &str) -> Result<(Module, Vec<Diagnostic>), Vec<Diagnostic>> {
    let (module, mut diagnostics) = tds_parser::parse_module(src);
    if !has_errors(&diagnostics) {
        diagnostics.extend(check_module(&module));
    }
    sort(&mut diagnostics);
    if has_errors(&diagnostics) {
        Err(diagnostics)
    } else {
        Ok((module, diagnostics))
    }
}

/// The semantic checks run after a successful parse.
fn check_module(module: &Module) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    if !module.items.iter().any(|item| item.name().name == "main") {
        diagnostics.push(
            Diagnostic::error(codes::NO_MAIN, "this program has no `main` function")
                .with_note("a program starts running at `fn main()`")
                .with_help("add one:\n\nfn main():\n    print(\"Hello!\")"),
        );
    }
    diagnostics
}

/// Checks and runs a program, writing its output to `out`. Returns the
/// warnings on success, or the compile errors / runtime panic.
pub fn run(
    src: &str,
    out: &mut dyn Write,
    args: Vec<String>,
) -> Result<Vec<Diagnostic>, Vec<Diagnostic>> {
    let (module, warnings) = check(src)?;
    let mut interp = Interpreter::new(out, args);
    interp.load_items(&module.items);
    let main_span = module
        .items
        .iter()
        .find(|item| item.name().name == "main")
        .map_or(Span::empty(0), |item| item.name().span);
    match interp.run_main(main_span) {
        Ok(()) => Ok(warnings),
        Err(panic) => Err(vec![panic]),
    }
}

pub fn has_errors(diagnostics: &[Diagnostic]) -> bool {
    diagnostics.iter().any(|d| d.severity == Severity::Error)
}

/// Orders diagnostics by where they occur in the file.
fn sort(diagnostics: &mut [Diagnostic]) {
    diagnostics.sort_by_key(|d| d.primary_span().map_or(0, |s| s.start));
}

/// Runs `f` on a thread with a stack big enough for deep recursion in the
/// interpreter.
pub fn with_big_stack<T: Send>(f: impl FnOnce() -> T + Send) -> T {
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(tds_interp::STACK_SIZE)
            .spawn_scoped(scope, f)
            .expect("failed to start the interpreter thread")
            .join()
            .unwrap_or_else(|e| std::panic::resume_unwind(e))
    })
}

/// Renders diagnostics in the rustc style:
///
/// ```text
/// error[TDS0412]: `player` was moved and can't be used here
///   ┌─ main.tds:9:11
///   │
/// 9 │     print(player.health)
///   │           ^^^^^^ used after the move
///   │
///   = help: ...
/// ```
pub fn render(diagnostics: &[Diagnostic], file_name: &str, src: &str, color: bool) -> String {
    let file = SimpleFile::new(file_name, src);
    let config = Config::default();
    let mut out = String::new();
    for diagnostic in diagnostics {
        let cs = to_codespan(diagnostic, src.len());
        if color {
            let mut buffer = termcolor::Buffer::ansi();
            let styles = Styles::default();
            let mut writer = StylesWriter::new(&mut buffer, &styles);
            term::emit_to_write_style(&mut writer, &config, &file, &cs)
                .expect("rendering a diagnostic");
            out.push_str(&String::from_utf8_lossy(buffer.as_slice()));
        } else {
            term::emit_to_string(&mut out, &config, &file, &cs).expect("rendering a diagnostic");
        }
    }
    out
}

fn to_codespan(diagnostic: &Diagnostic, src_len: usize) -> CsDiagnostic<()> {
    let severity = match diagnostic.severity {
        Severity::Error => CsSeverity::Error,
        Severity::Warning => CsSeverity::Warning,
    };
    let labels = diagnostic
        .labels
        .iter()
        .map(|label| {
            let range = label.span.start.min(src_len)..label.span.end.min(src_len);
            let cs = if label.primary {
                CsLabel::primary((), range)
            } else {
                CsLabel::secondary((), range)
            };
            cs.with_message(&label.message)
        })
        .collect();
    let notes = diagnostic
        .notes
        .iter()
        .map(|n| format!("note: {n}"))
        .chain(diagnostic.help.iter().map(|h| format!("help: {h}")))
        .collect();
    CsDiagnostic::new(severity)
        .with_code(diagnostic.code)
        .with_message(&diagnostic.message)
        .with_labels(labels)
        .with_notes(notes)
}
