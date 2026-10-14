//! Tests for the REPL: session behavior, multi-line entry detection, and
//! the `tds repl` command with piped input.

use std::cell::RefCell;
use std::io::Write;
use std::rc::Rc;

use tds_cli::repl::{needs_more_input, Session};

/// An output buffer the test can read while the session holds a writer.
#[derive(Clone, Default)]
struct Shared(Rc<RefCell<Vec<u8>>>);

impl Write for Shared {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.borrow_mut().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Shared {
    fn take(&self) -> String {
        String::from_utf8(std::mem::take(&mut *self.0.borrow_mut())).unwrap()
    }
}

/// Feeds entries one by one; returns each entry's output and error codes.
fn session(entries: &[&str]) -> Vec<(String, Vec<&'static str>)> {
    let shared = Shared::default();
    let mut writer = shared.clone();
    let mut session = Session::new(&mut writer);
    entries
        .iter()
        .map(|entry| {
            let outcome = session.eval(entry);
            let codes = outcome.diagnostics.iter().map(|d| d.code).collect();
            (shared.take(), codes)
        })
        .collect()
}

fn outputs(entries: &[&str]) -> Vec<String> {
    session(entries)
        .into_iter()
        .map(|(out, codes)| {
            if codes.is_empty() {
                out
            } else {
                format!("{out}{codes:?}")
            }
        })
        .collect()
}

#[test]
fn expressions_print_their_values() {
    assert_eq!(
        outputs(&[
            "1 + 2",
            "\"text\"",
            "'c'",
            "[1.5, 2.0]",
            "none ?? 3",
            "print(\"plain\")"
        ]),
        [
            "3\n",
            "\"text\"\n",
            "'c'\n",
            "[1.5, 2.0]\n",
            "[\"TDS0221\"]",
            "plain\n"
        ]
    );
}

#[test]
fn variables_and_declarations_persist() {
    assert_eq!(
        outputs(&[
            "var count = 1",
            "count += 41",
            "count",
            "fn double(n: Int) -> Int = n * 2\n",
            "double(count)",
            "struct P:\n    x: Int\n\n",
            "P(x: count).x",
        ]),
        ["", "", "42\n", "", "84\n", "", "42\n"]
    );
}

#[test]
fn errors_leave_the_session_unchanged() {
    assert_eq!(
        outputs(&["let a = 1", "let b = a + \"x\"", "b", "a"]),
        ["", "[\"TDS0213\"]", "[\"TDS0200\"]", "1\n"]
    );
}

#[test]
fn a_panic_rolls_back_the_entry() {
    // `c` was defined before the panic in the same entry, but the whole
    // entry is undone, including the move of `s`.
    assert_eq!(
        outputs(&[
            "let s = \"keep\"",
            "let c = take s\nlet boom = [1][3]\n",
            "c",
            "s",
        ]),
        ["", "[\"TDS0502\"]", "[\"TDS0200\"]", "\"keep\"\n"]
    );
}

#[test]
fn moves_are_tracked_across_entries() {
    assert_eq!(
        outputs(&["let s = \"x\"", "let t = take s", "s"]),
        ["", "", "[\"TDS0412\"]"]
    );
}

#[test]
fn redefinitions_replace_earlier_ones() {
    assert_eq!(
        outputs(&[
            "fn f() -> Int = 1\n",
            "fn f() -> String = \"two\"\n",
            "f()",
            "enum E:\n    A\n\n",
            "enum E:\n    B\n\n",
            "B",
            "A",
        ]),
        ["", "", "\"two\"\n", "", "", "B\n", "[\"TDS0200\"]"]
    );
}

#[test]
fn functions_cant_see_repl_variables() {
    assert_eq!(
        outputs(&["let secret = 1", "fn peek() -> Int = secret\n"]),
        ["", "[\"TDS0200\"]"]
    );
}

#[test]
fn return_at_top_level_is_an_error() {
    assert_eq!(outputs(&["return 1", "1"]), ["[\"TDS0238\"]", "1\n"]);
}

#[test]
fn if_without_else_at_top_level_runs() {
    assert_eq!(outputs(&["if true: print(1)"]), ["1\n"]);
}

#[test]
fn reset_forgets_everything() {
    let shared = Shared::default();
    let mut writer = shared.clone();
    let mut session = Session::new(&mut writer);
    session.eval("let x = 1");
    session.reset();
    let outcome = session.eval("x");
    assert_eq!(outcome.diagnostics[0].code, "TDS0200");
}

#[test]
fn entry_detection() {
    // Complete single lines.
    assert!(!needs_more_input("1 + 2\n"));
    assert!(!needs_more_input("let x = f(1)\n"));
    assert!(!needs_more_input("if x: print(1)\n"));
    // Open brackets and trailing operators continue.
    assert!(needs_more_input("let xs = [1,\n"));
    assert!(needs_more_input("let x = 1 +\n"));
    assert!(!needs_more_input("let x = 1 +\n    2\n"));
    assert!(!needs_more_input("let xs = [1,\n  2]\n"));
    // A block continues until an empty line.
    assert!(needs_more_input("fn f():\n"));
    assert!(needs_more_input("fn f():\n    return 1\n"));
    assert!(!needs_more_input("fn f():\n    return 1\n\n"));
    assert!(needs_more_input(
        "while x:\n    a()\n    if y:\n        b()\n"
    ));
    // A broken string doesn't trap the prompt.
    assert!(!needs_more_input("print(\"oops)\n"));
}

#[test]
fn repl_command_with_piped_input() {
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_tds"))
        .arg("repl")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"fn sq(n: Int) -> Int:\n    return n * n\n\nsq(7)\nnope\n:quit\nsq(1)\n")
        .unwrap();
    let out = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success());
    assert!(stdout.contains("49\n"), "{stdout}");
    assert!(
        !stdout.contains("1\n>>>"),
        ":quit should stop before sq(1): {stdout}"
    );
    assert!(
        stderr.contains("error[TDS0200]: unknown name `nope`"),
        "{stderr}"
    );
}
