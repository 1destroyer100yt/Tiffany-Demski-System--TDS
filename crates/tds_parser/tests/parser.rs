//! Parser tests. Successful parses are snapshot-tested through the AST
//! pretty-printer; failures check the error codes.

use tds_ast::{dump_module, dump_repl};
use tds_parser::{parse_module, parse_repl};

/// Parses a file that must have no errors and returns the AST dump.
#[track_caller]
fn parse_ok(src: &str) -> String {
    let (module, errors) = parse_module(src);
    assert!(errors.is_empty(), "unexpected errors: {errors:#?}");
    dump_module(&module)
}

/// Parses a statement list wrapped in `fn main():` and returns its dump.
#[track_caller]
fn stmts(src: &str) -> String {
    let mut wrapped = String::from("fn main():\n");
    for line in src.lines() {
        wrapped.push_str("    ");
        wrapped.push_str(line);
        wrapped.push('\n');
    }
    parse_ok(&wrapped)
}

/// The S-expression of a single expression.
#[track_caller]
fn expr(src: &str) -> String {
    let (entries, errors) = parse_repl(src);
    assert!(errors.is_empty(), "unexpected errors: {errors:#?}");
    dump_repl(&entries).trim_end().to_string()
}

fn error_codes(src: &str) -> Vec<&'static str> {
    parse_module(src).1.iter().map(|e| e.code).collect()
}

#[track_caller]
fn assert_errors(src: &str, expected: &[&str]) {
    assert_eq!(error_codes(src), expected, "\nsource:\n{src}");
}

// ======================================================================
// Expressions and precedence
// ======================================================================

#[test]
fn arithmetic_precedence() {
    assert_eq!(expr("1 + 2 * 3"), "(+ 1 (* 2 3))");
    assert_eq!(expr("(1 + 2) * 3"), "(* (+ 1 2) 3)");
    assert_eq!(expr("1 - 2 - 3"), "(- (- 1 2) 3)");
    assert_eq!(expr("8 / 4 / 2"), "(/ (/ 8 4) 2)");
    assert_eq!(expr("a % b * c"), "(* (% a b) c)");
}

#[test]
fn unary_operators() {
    assert_eq!(expr("-a * b"), "(* (- a) b)");
    assert_eq!(expr("--a"), "(- (- a))");
    assert_eq!(expr("-x.abs()"), "(- (method abs x))");
    assert_eq!(expr("not a == b"), "(not (== a b))");
    assert_eq!(expr("not a and b"), "(and (not a) b)");
}

#[test]
fn logical_precedence() {
    assert_eq!(expr("a or b and c"), "(or a (and b c))");
    assert_eq!(expr("a and b or c and d"), "(or (and a b) (and c d))");
    assert_eq!(expr("a < b and c >= d"), "(and (< a b) (>= c d))");
}

#[test]
fn coalesce_is_right_associative_and_binds_tighter_than_comparison() {
    assert_eq!(expr("a ?? b ?? c"), "(?? a (?? b c))");
    assert_eq!(expr("x > best ?? 0"), "(> x (?? best 0))");
    assert_eq!(expr("cfg.port ?? 8080"), "(?? (. cfg port) 8080)");
}

#[test]
fn ranges() {
    assert_eq!(expr("0..<10"), "(..< 0 10)");
    assert_eq!(expr("1...n + 1"), "(... 1 (+ n 1))");
}

#[test]
fn postfix_chains() {
    assert_eq!(expr("a.b.c"), "(. (. a b) c)");
    assert_eq!(expr("xs[i + 1]"), "(index xs (+ i 1))");
    assert_eq!(expr("f(x)(y)"), "(call (call f x) y)");
    assert_eq!(expr("grid[i][j]"), "(index (index grid i) j)");
    assert_eq!(expr("xs.pop()!"), "(! (method pop xs))");
    assert_eq!(expr("item > best!"), "(> item (! best))");
}

#[test]
fn literals() {
    assert_eq!(expr("42"), "42");
    assert_eq!(expr("2.5"), "2.5");
    assert_eq!(expr("true"), "true");
    assert_eq!(expr("none"), "none");
    assert_eq!(expr("()"), "()");
    assert_eq!(expr("'x'"), "'x'");
    assert_eq!(expr("\"hi\""), "\"hi\"");
    assert_eq!(expr("[]"), "(list)");
    assert_eq!(expr("[1, 2, 3,]"), "(list 1 2 3)");
}

#[test]
fn string_interpolation() {
    assert_eq!(expr(r#""Hello, {name}!""#), r#"(str "Hello, " {name} "!")"#);
    assert_eq!(expr(r#""{a + b * 2}""#), "(str {(+ a (* b 2))})");
    assert_eq!(
        expr(r#""{p.name} has {len(xs)}""#),
        r#"(str {(. p name)} " has " {(call len xs)})"#
    );
}

#[test]
fn call_arguments_with_modes_and_labels() {
    assert_eq!(
        expr("damage(mut player, 25)"),
        "(call damage (mut player) 25)"
    );
    assert_eq!(expr("save(take player)"), "(call save (take player))");
    assert_eq!(
        expr("Player(name: \"Alex\", health: 100)"),
        r#"(call Player (name: "Alex") (health: 100))"#
    );
    assert_eq!(expr("f(x, label: take y)"), "(call f x (label: (take y)))");
}

#[test]
fn take_expression() {
    assert_eq!(expr("let b = take a"), "let b = (take a)");
}

#[test]
fn inline_if_expression() {
    assert_eq!(
        expr("let label = if score >= 100: \"high\" else: \"low\""),
        r#"let label = if (>= score 100)
  "high"
else
  "low""#
    );
}

#[test]
fn nested_inline_if_in_expression() {
    assert_eq!(
        expr("let x = 1 + (if a: 2 else: 3)"),
        "let x = (+ 1 (if a 2 3))"
    );
}

// ======================================================================
// Statements
// ======================================================================

#[test]
fn let_var_and_assignment() {
    insta::assert_snapshot!(stmts(
        "let name = \"Tiffany\"\nvar score: Int = 100\nscore += 10\nscore = score * 2\nxs[0] = 1\np.health -= 5"
    ));
}

#[test]
fn if_else_chain() {
    insta::assert_snapshot!(stmts(
        "if a:\n    x()\nelse if b:\n    y()\nelse:\n    z()\nafter()"
    ));
}

#[test]
fn inline_if_with_else_on_next_line() {
    insta::assert_snapshot!(stmts("if a: x()\nelse: y()\nafter()"));
}

#[test]
fn if_as_value_with_blocks() {
    insta::assert_snapshot!(stmts(
        "let x = if a:\n    let t = 1\n    t + 1\nelse:\n    0\nprint(x)"
    ));
}

#[test]
fn loops() {
    insta::assert_snapshot!(stmts(
        "while running:\n    tick()\nfor i in 0..<10:\n    if i == 5: break\n    if i % 2 == 0: continue\n    print(i)\nfor x in xs: print(x)"
    ));
}

#[test]
fn return_forms() {
    insta::assert_snapshot!(stmts("if done: return\nreturn 1 + 2"));
}

#[test]
fn match_inline_and_block_arms() {
    insta::assert_snapshot!(stmts(
        "match s:\n    Circle(r): 3.14 * r * r\n    Rect(w, h):\n        let a = w * h\n        a\n    Point: 0.0\n    _: 1.0"
    ));
}

#[test]
fn match_as_value_and_literal_patterns() {
    insta::assert_snapshot!(stmts(
        "let name = match n:\n    0: \"zero\"\n    -1: \"minus one\"\n    other: \"many\"\nprint(name)"
    ));
}

// ======================================================================
// Items
// ======================================================================

#[test]
fn functions() {
    insta::assert_snapshot!(parse_ok(
        "fn greet(name: String) -> String:\n    return \"Hello, {name}!\"\n\nfn square(x: Int) -> Int = x * x\n\nfn damage(player: mut Player, amount: Int):\n    player.health -= amount\n\nfn consume(p: take Player): print(p.name)\n"
    ));
}

#[test]
fn structs_and_enums() {
    insta::assert_snapshot!(parse_ok(
        "struct Motor:\n    name: String\n    voltage: Float\n    running: Bool = false\n    tags: List[String]\n    next: Int?\n\nenum Shape:\n    Circle(radius: Float)\n    Rect(w: Float, h: Float)\n    Pair(Int, Int)\n    Point\n"
    ));
}

#[test]
fn pub_is_accepted() {
    insta::assert_snapshot!(parse_ok("pub fn f():\n    g()\n"));
}

#[test]
fn whole_program() {
    insta::assert_snapshot!(parse_ok(
        r#"// Player example from the brief.
struct Player:
    name: String
    health: Int

fn damage(player: mut Player, amount: Int):
    player.health -= amount

fn area(s: Shape) -> Float:
    match s:
        Circle(r): 3.14159 * r * r
        Rect(w, h): w * h
        Point: 0.0

fn main():
    var player = Player(name: "Alex", health: 100)
    damage(mut player, 25)
    print(player.health)
    let total = [1, 2, 3].len() +
        10
    for i in 1...3:
        print("{i}: {area(Circle(1.0))}")
"#
    ));
}

// ======================================================================
// Errors and recovery
// ======================================================================

#[test]
fn statement_at_top_level() {
    assert_errors("print(1)\n", &["TDS0104"]);
}

#[test]
fn missing_expression() {
    assert_errors("fn f():\n    let x = \n", &["TDS0100"]);
    assert_errors("fn f():\n    g(1 +)\n", &["TDS0100"]);
}

#[test]
fn missing_colon_after_condition() {
    assert_errors("fn f():\n    if x\n        y()\n", &["TDS0101"]);
}

#[test]
fn missing_block_after_colon() {
    assert_errors("fn f():\n    if x:\n    y()\n", &["TDS0102"]);
}

#[test]
fn unexpected_indent() {
    assert_errors("fn f():\n    a()\n        b()\n    c()\n", &["TDS0103"]);
}

#[test]
fn unexpected_indent_at_top_level_with_bad_width_reports_both() {
    // The lexer reports the width; the parser reports the stray block.
    assert_errors("fn f():\n    a()\n  b()\n", &["TDS0002"]);
}

#[test]
fn invalid_assignment_target() {
    assert_errors("fn f():\n    f() = 1\n", &["TDS0105"]);
    assert_errors("fn f():\n    1 + 2 = 3\n", &["TDS0105"]);
}

#[test]
fn not_supported_features() {
    assert_errors("impl Motor:\n    fn f(self): x\n", &["TDS0106"]);
    assert_errors("pub use std.io\n", &["TDS0106"]);
    assert_errors(
        "fn largest[T](xs: List[T]) -> T:\n    xs[0]\n",
        &["TDS0106"],
    );
    assert_errors("fn f() -> Int:\n    return g()?\n", &["TDS0106"]);
    assert_errors("@derive(Equal)\nstruct P:\n    x: Int\n", &["TDS0106"]);
}

#[test]
fn chained_comparison() {
    assert_errors("fn f():\n    if 0 < x < 10: g()\n", &["TDS0107"]);
}

#[test]
fn missing_type() {
    assert_errors("fn f(x: ) :\n    g()\n", &["TDS0108"]);
    assert_errors("fn f():\n    let x: = 1\n", &["TDS0108"]);
}

#[test]
fn positional_after_named() {
    assert_errors("fn f():\n    P(x: 1, 2)\n", &["TDS0110"]);
}

#[test]
fn missing_parameter_type() {
    assert_errors("fn f(x):\n    g()\n", &["TDS0111"]);
}

#[test]
fn else_without_if() {
    assert_errors("fn f():\n    g()\n    else:\n        h()\n", &["TDS0112"]);
}

#[test]
fn assignment_in_condition() {
    assert_errors("fn f():\n    if x = 1:\n        g()\n", &["TDS0113"]);
}

#[test]
fn missing_initializer() {
    assert_errors("fn f():\n    var x: Int\n", &["TDS0114"]);
}

#[test]
fn misplaced_mode() {
    assert_errors("fn f():\n    let x = mut y\n", &["TDS0115"]);
    assert_errors("fn f():\n    let x: mut Int = 1\n", &["TDS0115"]);
}

#[test]
fn expected_pattern() {
    assert_errors("fn f():\n    match x:\n        +: 1\n", &["TDS0116"]);
}

#[test]
fn errors_in_interpolation() {
    assert_errors("fn f():\n    print(\"{1 +}\")\n", &["TDS0100"]);
    assert_errors("fn f():\n    print(\"{a b}\")\n", &["TDS0101"]);
}

#[test]
fn one_typo_gives_one_error() {
    // Each broken line is reported once and the rest still parses.
    let src = "fn f():\n    let x = = 1\n    let y = 2\n    if y $ 3:\n        z()\n    w()\n\nfn g():\n    ok()\n";
    let (module, errors) = parse_module(src);
    let codes: Vec<_> = errors.iter().map(|e| e.code).collect();
    assert_eq!(codes, ["TDS0004", "TDS0100"]);
    assert_eq!(module.items.len(), 2);
    insta::assert_snapshot!(dump_module(&module));
}

#[test]
fn lexer_errors_are_not_duplicated_by_the_parser() {
    assert_errors("fn f():\n    let s = \"abc\n", &["TDS0005"]);
    assert_errors("fn f():\n    g(1, 2\n", &["TDS0010"]);
}

#[test]
fn brief_examples_within_phase_1_parse() {
    // Everything from the syntax tour that's in the Phase 1 subset.
    parse_ok(
        r#"fn greet(name: String) -> String:
    return "Hello, {name}!"

fn square(x: Int) -> Int = x * x

fn main():
    let name = "Tiffany"
    var score: Int = 100
    score += 10
    let label = if score >= 100: "high" else: "low"
    for i in 0..<10:
        print(i)
    for i in 1...10:
        print(i)
    while running:
        tick()

enum Shape:
    Circle(radius: Float)
    Rect(w: Float, h: Float)
    Point

fn area(s: Shape) -> Float:
    match s:
        Circle(r): 3.14159 * r * r
        Rect(w, h): w * h
        Point: 0.0

struct Motor:
    name: String
    voltage: Float
    max_rpm: Int
    running: Bool = false

fn largest(items: List[Int]) -> Int?:
    var best: Int? = none
    for item in items:
        if best == none or item > best!:
            best = item
    return best
"#,
    );
}

#[test]
fn use_declarations() {
    assert_eq!(
        parse_ok("use motors\nuse motors.types\nuse motors.types.{Motor, Shape}\n\nfn main():\n    print(1)\n"),
        "use motors\nuse motors.types\nuse motors.types.{Motor, Shape}\nfn main() -> ()\n  (call print 1)\n"
    );
}

#[test]
fn dotted_type_names() {
    assert_eq!(
        parse_ok("fn f(m: types.Motor) -> List[a.b.C]?:\n    g()\n"),
        "fn f(m: types.Motor) -> List[a.b.C]?\n  (call g)\n"
    );
}

#[test]
fn bad_use_declarations() {
    assert_errors("use\n", &["TDS0101"]);
    assert_errors("use a.{B\n", &["TDS0010"]);
}
