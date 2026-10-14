//! Runs every program in `tests/run-pass/` and compares its output with
//! the `.out` file next to it. Each program gets its own path as its first
//! command-line argument (the file-reader test reads itself).
//!
//! A program with modules is a directory with `main.tds` and
//! `expected.out`.

use std::path::{Path, PathBuf};

fn run_pass_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/run-pass")
}

#[test]
fn run_pass_programs() {
    let mut programs: Vec<PathBuf> = std::fs::read_dir(run_pass_dir())
        .expect("tests/run-pass exists")
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.extension().is_some_and(|ext| ext == "tds") || path.join("main.tds").exists()
        })
        .collect();
    programs.sort();
    assert!(
        programs.len() >= 8,
        "expected the run-pass programs, found {}",
        programs.len()
    );

    let mut failures = Vec::new();
    for path in &programs {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let (main_file, expected_file) = if path.is_dir() {
            (path.join("main.tds"), path.join("expected.out"))
        } else {
            (path.clone(), path.with_extension("out"))
        };
        let expected = std::fs::read_to_string(&expected_file)
            .unwrap_or_else(|_| panic!("{name} has no expected output file"));

        let args = vec![path.to_string_lossy().to_string()];
        let (output, result) = tds_cli::with_big_stack(|| {
            let mut output = Vec::new();
            let result = match tds_cli::compile_file(&main_file) {
                Ok(checked) => match tds_cli::interpret(&checked, &mut output, args) {
                    Ok(()) => Ok((checked.warnings, checked.sources)),
                    Err(panic) => Err((vec![panic], checked.sources)),
                },
                Err(failure) => Err((failure.diagnostics, failure.sources)),
            };
            (String::from_utf8(output).unwrap(), result)
        });

        match result {
            Err((errors, sources)) => failures.push(format!(
                "{name} failed:\n{}",
                tds_cli::render_with(&errors, &sources, false)
            )),
            Ok((warnings, sources)) if !warnings.is_empty() => failures.push(format!(
                "{name} had warnings:\n{}",
                tds_cli::render_with(&warnings, &sources, false)
            )),
            Ok(_) if output != expected => failures.push(format!(
                "{name}: wrong output\n--- expected\n{expected}--- actual\n{output}"
            )),
            Ok(_) => {}
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}
