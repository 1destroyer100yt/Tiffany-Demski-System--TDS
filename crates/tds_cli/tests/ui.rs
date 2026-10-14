//! UI tests: each file in `tests/ui/` is a program with a mistake. Its
//! name starts with the error code it must produce, like
//! `TDS0412-use-after-move.tds`. The rendered diagnostics are compared
//! with an `insta` snapshot in `tests/ui/snapshots/`, so any change to an
//! error message shows up in review.
//!
//! Programs that pass the checker are run, so runtime panics are covered
//! the same way.
//!
//! To accept changed output: `INSTA_UPDATE=always cargo test -p tds_cli --test ui`,
//! then review the diff with git.

use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn ui_files() -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(root().join("tests/ui"))
        .expect("tests/ui exists")
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "tds"))
        .collect();
    files.sort();
    files
}

/// Checks (and, if that succeeds, runs) a program. Returns the rendered
/// output and the codes of all diagnostics.
fn compile_and_run(name: &str, src: &str) -> (String, Vec<&'static str>) {
    let src = src.to_string();
    let name = name.to_string();
    tds_cli::with_big_stack(move || {
        let mut stdout = Vec::new();
        let diagnostics = match tds_cli::run(&src, &mut stdout, Vec::new()) {
            Ok(warnings) => warnings,
            Err(errors) => errors,
        };
        let mut output = tds_cli::render(&diagnostics, &name, &src, false);
        if !stdout.is_empty() {
            output.push_str("--- program output ---\n");
            output.push_str(&String::from_utf8_lossy(&stdout));
        }
        (output, diagnostics.iter().map(|d| d.code).collect())
    })
}

#[test]
fn ui_tests() {
    let files = ui_files();
    assert!(
        files.len() >= 50,
        "expected at least 50 UI tests, found {}",
        files.len()
    );

    let mut failures = Vec::new();
    for path in &files {
        let stem = path.file_stem().unwrap().to_string_lossy().to_string();
        let file_name = format!("{stem}.tds");
        let expected_code = &stem[..7];
        let src = std::fs::read_to_string(path).unwrap();
        let (output, codes) = compile_and_run(&file_name, &src);

        if !codes.contains(&expected_code) {
            failures.push(format!(
                "{file_name}: expected {expected_code}, got {codes:?}\n{output}"
            ));
            continue;
        }
        // One mistake should give one diagnostic.
        if codes.len() != 1 {
            failures.push(format!(
                "{file_name}: expected exactly one diagnostic, got {codes:?}\n{output}"
            ));
            continue;
        }

        insta::with_settings!({
            snapshot_path => root().join("tests/ui/snapshots"),
            prepend_module_to_snapshot => false,
            omit_expression => true,
            description => src.clone(),
        }, {
            insta::assert_snapshot!(stem.clone(), output);
        });
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

/// Every error code in the registry has a UI test.
#[test]
fn every_code_has_a_ui_test() {
    let registry =
        std::fs::read_to_string(root().join("crates/tds_diagnostics/src/codes.rs")).unwrap();
    let tested: Vec<String> = ui_files()
        .iter()
        .map(|p| p.file_stem().unwrap().to_string_lossy()[..7].to_string())
        .collect();
    let mut missing = Vec::new();
    for code in registry
        .split('"')
        .filter(|s| s.len() == 7 && s.starts_with("TDS"))
    {
        // Internal errors mean a compiler bug, so no program should hit them.
        if code == tds_diagnostics::codes::INTERNAL_ERROR {
            continue;
        }
        if !tested.iter().any(|t| t == code) {
            missing.push(code);
        }
    }
    assert!(
        missing.is_empty(),
        "error codes without a UI test: {missing:?}"
    );
}
