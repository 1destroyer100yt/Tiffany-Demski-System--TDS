//! End-to-end tests of the `tds` binary: commands, exit codes, output.

use std::process::{Command, Output};

fn tds(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_tds"))
        .args(args)
        .output()
        .expect("failed to run tds")
}

fn scratch_file(name: &str, contents: &str) -> String {
    let dir = std::env::temp_dir().join(format!("tds-cli-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, contents).unwrap();
    path.to_string_lossy().to_string()
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).to_string()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).to_string()
}

#[test]
fn run_prints_program_output() {
    let path = scratch_file("hello.tds", "fn main():\n    print(\"Hello from TDS\")\n");
    let out = tds(&["run", &path]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(stdout(&out), "Hello from TDS\n");
}

#[test]
fn run_passes_arguments() {
    let path = scratch_file("args.tds", "fn main():\n    print(args())\n");
    let out = tds(&["run", &path, "one", "two"]);
    assert_eq!(stdout(&out), "[\"one\", \"two\"]\n");
}

#[test]
fn run_reports_compile_errors_and_fails() {
    let path = scratch_file("bad.tds", "fn main():\n    let x = 1\n    x = 2\n");
    let out = tds(&["run", &path]);
    assert!(!out.status.success());
    let err = stderr(&out);
    assert!(err.contains("error[TDS0400]"), "{err}");
    assert!(
        err.contains("1 error found. For more about an error, run `tds explain TDS0400`."),
        "{err}"
    );
    assert_eq!(stdout(&out), "");
}

#[test]
fn run_reports_panics_after_partial_output() {
    let path = scratch_file(
        "panic.tds",
        "fn main():\n    print(\"before\")\n    let xs = [1]\n    print(xs[5])\n",
    );
    let out = tds(&["run", &path]);
    assert!(!out.status.success());
    assert_eq!(stdout(&out), "before\n");
    assert!(stderr(&out).contains("error[TDS0502]: index 5 is out of bounds"));
}

#[test]
fn check_succeeds_without_running() {
    let path = scratch_file("checked.tds", "fn main():\n    print(\"not printed\")\n");
    let out = tds(&["check", &path]);
    assert!(out.status.success());
    assert_eq!(stdout(&out), "");
}

#[test]
fn check_counts_several_errors() {
    let path = scratch_file(
        "two.tds",
        "fn main():\n    let a: Int = \"x\"\n    print(b)\n",
    );
    let out = tds(&["check", &path]);
    assert!(!out.status.success());
    assert!(
        stderr(&out)
            .contains("2 errors found. For more about an error, run `tds explain TDS0203`."),
        "{}",
        stderr(&out)
    );
}

#[test]
fn warnings_dont_fail_the_run() {
    let path = scratch_file(
        "warn.tds",
        "fn main():\n    match 1:\n        n: print(n)\n        2: print(2)\n",
    );
    let out = tds(&["run", &path]);
    assert!(out.status.success());
    assert_eq!(stdout(&out), "1\n");
    assert!(stderr(&out).contains("warning[TDS0239]"));
}

#[test]
fn explain_prints_the_long_form() {
    let out = tds(&["explain", "tds0412"]);
    assert!(out.status.success());
    assert!(stdout(&out).starts_with("TDS0412\n"));
    assert!(stdout(&out).contains("take player.clone()"));
}

#[test]
fn explain_rejects_unknown_codes() {
    let out = tds(&["explain", "TDS9999"]);
    assert!(!out.status.success());
}

#[test]
fn parse_and_tokens_commands() {
    let path = scratch_file("tiny.tds", "fn main():\n    print(1 + 2 * 3)\n");
    let out = tds(&["parse", &path]);
    assert!(
        stdout(&out).contains("(call print (+ 1 (* 2 3)))"),
        "{}",
        stdout(&out)
    );
    let out = tds(&["tokens", &path]);
    assert!(stdout(&out).contains("INDENT"));
}

#[test]
fn missing_file_and_unknown_command() {
    assert!(!tds(&["run", "/no/such/file.tds"]).status.success());
    assert!(!tds(&["frobnicate"]).status.success());
    assert!(!tds(&[]).status.success());
    assert!(tds(&["help"]).status.success());
}
