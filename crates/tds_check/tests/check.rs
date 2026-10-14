//! Checker tests focused on the move analysis and on programs that must
//! be accepted (no false positives). Every error code also has a UI test
//! in `tests/ui/`.

use tds_check::check_module;
use tds_parser::parse_module;

fn codes(src: &str) -> Vec<&'static str> {
    let (module, parse_errors) = parse_module(src);
    assert!(parse_errors.is_empty(), "parse errors: {parse_errors:#?}");
    check_module(&module).iter().map(|d| d.code).collect()
}

/// Wraps statements in `fn main():` after some shared declarations.
fn program(body: &str) -> String {
    let mut src = String::from(
        "struct Item:\n    name: String\n\nfn consume(i: take Item):\n    print(i.name)\n\nfn look(i: Item):\n    print(i.name)\n\nfn main():\n",
    );
    for line in body.lines() {
        src.push_str("    ");
        src.push_str(line);
        src.push('\n');
    }
    src
}

#[track_caller]
fn ok(body: &str) {
    let src = program(body);
    assert_eq!(codes(&src), Vec::<&str>::new(), "\n{src}");
}

#[track_caller]
fn err(body: &str, expected: &[&str]) {
    let src = program(body);
    assert_eq!(codes(&src), expected, "\n{src}");
}

// ---- moves ----

#[test]
fn use_after_take() {
    err(
        "let a = Item(name: \"a\")\nconsume(take a)\nlook(a)",
        &["TDS0412"],
    );
}

#[test]
fn reassigning_a_moved_var_revives_it() {
    ok("var a = Item(name: \"a\")\nconsume(take a)\na = Item(name: \"b\")\nlook(a)");
}

#[test]
fn use_after_move_is_reported_once() {
    err(
        "let a = Item(name: \"a\")\nconsume(take a)\nlook(a)\nlook(a)",
        &["TDS0412"],
    );
}

#[test]
fn move_in_one_branch_counts_after_the_if() {
    err(
        "let a = Item(name: \"a\")\nif true:\n    consume(take a)\nlook(a)",
        &["TDS0412"],
    );
}

#[test]
fn move_in_both_branches_then_no_use_is_fine() {
    ok("let a = Item(name: \"a\")\nif true:\n    consume(take a)\nelse:\n    consume(take a)");
}

#[test]
fn branch_that_returns_doesnt_leak_its_moves() {
    ok("let a = Item(name: \"a\")\nif true:\n    consume(take a)\n    return\nlook(a)");
}

#[test]
fn move_in_a_match_arm() {
    err(
        "let a = Item(name: \"a\")\nmatch 1:\n    1: consume(take a)\n    _: print(0)\nlook(a)",
        &["TDS0412"],
    );
}

#[test]
fn move_in_loop_then_break_is_fine() {
    ok("let a = Item(name: \"a\")\nwhile true:\n    consume(take a)\n    break");
}

#[test]
fn move_in_loop_is_reported() {
    err(
        "let a = Item(name: \"a\")\nfor i in 0..<2:\n    consume(take a)",
        &["TDS0411"],
    );
}

#[test]
fn move_in_loop_with_reassignment_is_fine() {
    ok("var a = Item(name: \"a\")\nfor i in 0..<2:\n    consume(take a)\n    a = Item(name: \"again\")");
}

#[test]
fn move_of_a_variable_declared_inside_the_loop_is_fine() {
    ok("for i in 0..<2:\n    let a = Item(name: \"a\")\n    consume(take a)");
}

#[test]
fn use_after_loop_that_moved() {
    err(
        "let a = Item(name: \"a\")\nwhile false:\n    consume(take a)\n    break\nlook(a)",
        &["TDS0412"],
    );
}

#[test]
fn copy_types_are_never_moved() {
    ok("let n = 5\nlet m = take n\nprint(n + m)");
}

#[test]
fn take_in_short_circuit_rhs_is_maybe_moved() {
    err("let a = Item(name: \"a\")\nfn_ok(a)\n", &["TDS0200"]);
    let src = "struct Item:\n    name: String\n\nfn eat(i: take Item) -> Bool = true\n\nfn main():\n    let a = Item(name: \"a\")\n    if false and eat(take a):\n        print(1)\n    print(a.name)\n";
    assert_eq!(codes(src), ["TDS0412"]);
}

#[test]
fn moves_inside_functions_are_independent() {
    let src = "struct Item:\n    name: String\n\nfn eat(i: take Item):\n    print(i.name)\n\nfn a(x: take Item):\n    eat(take x)\n\nfn b(x: take Item):\n    eat(take x)\n\nfn main():\n    a(Item(name: \"1\"))\n";
    assert_eq!(codes(src), Vec::<&str>::new());
}

#[test]
fn take_of_a_temporary_is_fine() {
    ok("consume(take Item(name: \"t\"))\nconsume(Item(name: \"u\"))");
}

#[test]
fn moving_into_a_struct_field() {
    let src = "struct Item:\n    name: String\n\nstruct Box2:\n    item: Item\n\nfn main():\n    let i = Item(name: \"a\")\n    let b = Box2(item: take i)\n    print(i.name)\n";
    assert_eq!(codes(src), ["TDS0412"]);
}

#[test]
fn moving_into_a_list_with_push() {
    ok("var xs: List[Item] = []\nlet i = Item(name: \"a\")\nxs.push(take i)\nprint(len(xs))");
    err(
        "var xs: List[Item] = []\nlet i = Item(name: \"a\")\nxs.push(take i)\nlook(i)",
        &["TDS0412"],
    );
}

// ---- modes and mutability ----

#[test]
fn two_phase_mut_argument() {
    let src = "fn f(xs: mut List[Int], n: Int):\n    xs.push(n)\n\nfn main():\n    var xs = [1]\n    f(mut xs, len(xs))\n    f(mut xs, xs[0])\n";
    // Reading through `len(xs)` finishes before the call; passing `xs[0]`
    // directly as another argument is a conflict.
    assert_eq!(codes(src), ["TDS0410"]);
}

#[test]
fn mut_param_can_be_reborrowed_and_assigned() {
    let src = "fn inner(xs: mut List[Int]):\n    xs.push(1)\n\nfn outer(xs: mut List[Int]):\n    inner(mut xs)\n    xs[0] = 5\n\nfn main():\n    var xs = [0]\n    outer(mut xs)\n";
    assert_eq!(codes(src), Vec::<&str>::new());
}

#[test]
fn mut_argument_type_must_match_exactly() {
    let src = "fn set(x: mut Int?):\n    x = none\n\nfn main():\n    var n = 1\n    set(mut n)\n";
    assert_eq!(codes(src), ["TDS0203"]);
}

#[test]
fn take_param_is_owned_and_can_change() {
    let src = "struct Item:\n    name: String\n\nfn rename(i: take Item) -> Item:\n    i.name = \"new\"\n    return i\n\nfn main():\n    print(rename(Item(name: \"a\")).name)\n";
    assert_eq!(codes(src), Vec::<&str>::new());
}

#[test]
fn loop_variables_and_bindings_are_immutable() {
    err("for i in 0..<3:\n    i += 1", &["TDS0400"]);
    err("match 5:\n    n:\n        n = 1", &["TDS0400"]);
}

#[test]
fn copy_value_to_take_param_needs_no_marker() {
    let src = "fn keep(n: take Int):\n    print(n)\n\nfn main():\n    let n = 1\n    keep(n)\n";
    assert_eq!(codes(src), Vec::<&str>::new());
}

// ---- types ----

#[test]
fn optional_coercions() {
    ok("var x: Int? = 5\nx = none\nx = 3\nprint(x ?? 0)\nprint(x == 3)\nprint(none == x)");
}

#[test]
fn empty_list_takes_type_from_context() {
    ok("var xs: List[Int] = []\nxs = []\nprint(xs == [])\nprint([] == xs)");
}

#[test]
fn if_branches_must_agree_when_used_as_value() {
    err("let x = if true: 1 else: \"one\"", &["TDS0203"]);
    ok("if true:\n    print(1)\nelse:\n    look(Item(name: \"x\"))");
}

#[test]
fn never_type_fits_anywhere() {
    let src = "fn f(n: Int) -> String:\n    if n > 0:\n        return \"pos\"\n    else:\n        panic(\"no\")\n\nfn main():\n    print(f(1))\n";
    assert_eq!(codes(src), Vec::<&str>::new());
}

#[test]
fn match_with_all_returning_arms_has_type_never() {
    let src = "fn f(b: Bool) -> Int:\n    match b:\n        true: return 1\n        false: return 2\n\nfn main():\n    print(f(true))\n";
    assert_eq!(codes(src), Vec::<&str>::new());
}

#[test]
fn errors_dont_cascade_through_unknown_types() {
    // One unknown name; everything built from it stays quiet.
    err(
        "let x = mystery + 1\nlet y = x * 2\nprint(y.len())",
        &["TDS0200"],
    );
}

#[test]
fn shadowing_changes_type() {
    ok("let x = 1\nlet x = \"one\"\nprint(x.len())");
}

#[test]
fn enum_variants_and_qualified_names() {
    let src = "enum Dir:\n    North\n    South\n    Go(steps: Int)\n\nfn main():\n    let d = Dir.North\n    let e = Go(steps: 3)\n    let f = Dir.Go(1)\n    match d:\n        North: print(1)\n        South: print(2)\n        Go(n): print(n)\n";
    assert_eq!(codes(src), Vec::<&str>::new());
}

#[test]
fn nested_variant_patterns_need_a_catch_all() {
    let src = "enum Opt:\n    Some(n: Int)\n    Nothing\n\nfn main():\n    match Some(1):\n        Some(0): print(0)\n        Nothing: print(1)\n";
    // `Some(0)` doesn't cover every `Some`, so `Some(..)` is missing.
    assert_eq!(codes(src), ["TDS0217"]);
}

#[test]
fn mutually_recursive_structs_are_reported_once() {
    let src = "struct A:\n    b: B\n\nstruct B:\n    a: A\n\nfn main():\n    print(1)\n";
    assert_eq!(codes(src), ["TDS0232"]);
}

#[test]
fn recursion_through_a_list_is_fine() {
    let src = "struct Tree:\n    value: Int\n    children: List[Tree]\n\nfn main():\n    let t = Tree(value: 1, children: [])\n    print(t.value)\n";
    assert_eq!(codes(src), Vec::<&str>::new());
}
