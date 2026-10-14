//! The compiler driver: runs the pipeline stages in order and renders
//! diagnostics. The `tds` binary (`main.rs`) and the test suites both go
//! through these functions, so they always see the same behavior.
//!
//! Pipeline:
//!
//! ```text
//! files → lexer → parser → modules (tds_resolve) → checker (tds_check)
//!       → interpreter (tds run)
//! ```

pub mod repl;

use std::io::Write;
use std::path::Path;

use codespan_reporting::diagnostic::{
    Diagnostic as CsDiagnostic, Label as CsLabel, Severity as CsSeverity,
};
use codespan_reporting::files::SimpleFiles;
use codespan_reporting::term::{self, termcolor, Config, Styles, StylesWriter};
use tds_ast::Module;
use tds_diagnostics::{codes, Diagnostic, Severity, SourceMap, Span};
use tds_interp::Interpreter;
use tds_resolve::{DirLoader, Program};

/// A program that passed every check, ready to run or compile.
pub struct Checked {
    /// All modules merged into one, with expression ids assigned.
    pub module: Module,
    pub sources: SourceMap,
    pub warnings: Vec<Diagnostic>,
}

/// Compile errors, or a runtime panic, with the files needed to show them.
#[derive(Debug)]
pub struct Failure {
    pub sources: SourceMap,
    pub diagnostics: Vec<Diagnostic>,
}

impl Failure {
    pub fn render(&self, color: bool) -> String {
        render_with(&self.diagnostics, &self.sources, color)
    }

    pub fn codes(&self) -> Vec<&'static str> {
        self.diagnostics.iter().map(|d| d.code).collect()
    }
}

/// Loads and checks a program from its main file. Modules are looked up
/// in the main file's directory.
pub fn compile_file(path: &Path) -> Result<Checked, Failure> {
    let text = std::fs::read_to_string(path).map_err(|e| Failure {
        sources: SourceMap::new(),
        diagnostics: vec![Diagnostic::error(
            codes::UNKNOWN_MODULE,
            format!("can't read `{}`: {e}", path.display()),
        )],
    })?;
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let name = path.to_string_lossy().replace('\\', "/");
    compile_source(&name, &text, dir)
}

/// Loads and checks source text; `module_dir` is where its modules are
/// looked up.
pub fn compile_source(name: &str, src: &str, module_dir: &Path) -> Result<Checked, Failure> {
    // Module files are named in messages relative to the main file, the
    // way its own name was given.
    let display_prefix = match name.rfind('/') {
        Some(i) => name[..=i].to_string(),
        None => String::new(),
    };
    let loader = DirLoader {
        dir: module_dir.to_path_buf(),
        display_prefix,
    };
    let (program, diagnostics) = tds_resolve::load_program(name, src, &loader);
    check_program(program, diagnostics)
}

/// Runs the checker on a loaded program. `diagnostics` are the errors found
/// while loading it.
pub fn check_program(
    program: Program,
    mut diagnostics: Vec<Diagnostic>,
) -> Result<Checked, Failure> {
    let Program {
        mut module,
        sources,
    } = program;
    if !has_errors(&diagnostics) {
        tds_ast::number_exprs(&mut module);
        diagnostics.extend(tds_check::check_module(&module));
    }
    sort(&mut diagnostics);
    if has_errors(&diagnostics) {
        return Err(Failure {
            sources,
            diagnostics,
        });
    }
    Ok(Checked {
        module,
        sources,
        warnings: diagnostics,
    })
}

/// Runs a checked program with the interpreter. Returns the runtime panic,
/// if there is one.
pub fn interpret(
    checked: &Checked,
    out: &mut dyn Write,
    args: Vec<String>,
) -> Result<(), Diagnostic> {
    let mut interp = Interpreter::new(out, args);
    interp.load_items(&checked.module.items);
    let main_span = checked
        .module
        .items
        .iter()
        .find(|item| item.name().name == "main")
        .map_or(Span::empty(0), |item| item.name().span);
    interp.run_main(main_span)
}

/// Parses and checks a single source file that has no modules. On success
/// returns the module, ready to run, plus any warnings.
pub fn check(src: &str) -> Result<(Module, Vec<Diagnostic>), Vec<Diagnostic>> {
    match compile_source("main.tds", src, Path::new(".")) {
        Ok(checked) => Ok((checked.module, checked.warnings)),
        Err(failure) => Err(failure.diagnostics),
    }
}

/// Checks and runs a single source file, writing its output to `out`.
/// Returns the warnings on success, or the compile errors / runtime panic.
pub fn run(
    src: &str,
    out: &mut dyn Write,
    args: Vec<String>,
) -> Result<Vec<Diagnostic>, Vec<Diagnostic>> {
    let checked = compile_source("main.tds", src, Path::new(".")).map_err(|f| f.diagnostics)?;
    match interpret(&checked, out, args) {
        Ok(()) => Ok(checked.warnings),
        Err(panic) => Err(vec![panic]),
    }
}

pub fn has_errors(diagnostics: &[Diagnostic]) -> bool {
    diagnostics.iter().any(|d| d.severity == Severity::Error)
}

/// Orders diagnostics by where they occur in the program.
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

/// Renders diagnostics for a single file in the rustc style:
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
    render_with(diagnostics, &SourceMap::single(file_name, src), color)
}

/// Renders diagnostics whose spans may point into any file of a program.
pub fn render_with(diagnostics: &[Diagnostic], sources: &SourceMap, color: bool) -> String {
    let mut files = SimpleFiles::new();
    for file in sources.files() {
        files.add(file.name.clone(), file.text.clone());
    }
    let config = Config::default();
    let mut out = String::new();
    for diagnostic in diagnostics {
        let cs = to_codespan(diagnostic, sources);
        if color {
            let mut buffer = termcolor::Buffer::ansi();
            let styles = Styles::default();
            let mut writer = StylesWriter::new(&mut buffer, &styles);
            term::emit_to_write_style(&mut writer, &config, &files, &cs)
                .expect("rendering a diagnostic");
            out.push_str(&String::from_utf8_lossy(buffer.as_slice()));
        } else {
            term::emit_to_string(&mut out, &config, &files, &cs).expect("rendering a diagnostic");
        }
    }
    out
}

fn to_codespan(diagnostic: &Diagnostic, sources: &SourceMap) -> CsDiagnostic<usize> {
    let severity = match diagnostic.severity {
        Severity::Error => CsSeverity::Error,
        Severity::Warning => CsSeverity::Warning,
    };
    let labels = diagnostic
        .labels
        .iter()
        .filter_map(|label| {
            let file_id = sources.file_index(label.span.start)?;
            let file = &sources.files()[file_id];
            let len = file.text.len();
            let range =
                (label.span.start - file.base).min(len)..(label.span.end - file.base).min(len);
            let cs = if label.primary {
                CsLabel::primary(file_id, range)
            } else {
                CsLabel::secondary(file_id, range)
            };
            Some(cs.with_message(&label.message))
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
