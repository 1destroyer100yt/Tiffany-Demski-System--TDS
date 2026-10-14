//! Lexer tests.
//!
//! Most tests compare a compact dump of the token stream, one logical line
//! per entry (split after each NEWLINE), so INDENT/DEDENT placement is easy
//! to read at a glance.

use tds_lexer::{lex, StrPart, TokenKind};

/// Renders tokens as lines of text, starting a new line after each NEWLINE.
fn dump(src: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current: Vec<String> = Vec::new();
    for tok in lex(src).tokens {
        let is_newline = tok.kind == TokenKind::Newline;
        current.push(tok.kind.to_string());
        if is_newline {
            lines.push(current.join(" "));
            current.clear();
        }
    }
    if !current.is_empty() {
        lines.push(current.join(" "));
    }
    lines
}

/// Asserts the token dump and that there were no errors.
#[track_caller]
fn assert_tokens(src: &str, expected: &[&str]) {
    let out = lex(src);
    assert!(
        out.errors.is_empty(),
        "unexpected errors: {:#?}",
        out.errors
    );
    assert_eq!(dump(src), expected, "\nsource:\n{src}");
}

/// Error codes produced for `src`, in order.
fn error_codes(src: &str) -> Vec<&'static str> {
    lex(src).errors.iter().map(|e| e.code).collect()
}

/// Asserts the exact list of error codes.
#[track_caller]
fn assert_errors(src: &str, expected: &[&str]) {
    assert_eq!(error_codes(src), expected, "\nsource:\n{src}");
}

// ======================================================================
// Empty input and trivia
// ======================================================================

#[test]
fn empty_file() {
    assert_tokens("", &["EOF"]);
}

#[test]
fn only_blank_lines_and_comments() {
    assert_tokens(
        "\n\n   \n// a comment\n    // indented comment\n\n",
        &["EOF"],
    );
}

#[test]
fn single_line_without_trailing_newline() {
    assert_tokens("let x = 5", &["let ident(x) = int(5) NEWLINE", "EOF"]);
}

#[test]
fn single_line_with_trailing_newline() {
    assert_tokens("let x = 5\n", &["let ident(x) = int(5) NEWLINE", "EOF"]);
}

#[test]
fn trailing_comment_after_code() {
    assert_tokens(
        "let x = 5 // five\nlet y = 6",
        &[
            "let ident(x) = int(5) NEWLINE",
            "let ident(y) = int(6) NEWLINE",
            "EOF",
        ],
    );
}

#[test]
fn doc_comments_are_skipped() {
    assert_tokens("/// Adds.\nfn f()", &["fn ident(f) ( ) NEWLINE", "EOF"]);
}

#[test]
fn crlf_line_endings() {
    assert_tokens(
        "fn f():\r\n    return 1\r\n\r\nf()\r\n",
        &[
            "fn ident(f) ( ) : NEWLINE",
            "INDENT return int(1) NEWLINE",
            "DEDENT ident(f) ( ) NEWLINE",
            "EOF",
        ],
    );
}

// ======================================================================
// INDENT / DEDENT
// ======================================================================

#[test]
fn simple_block() {
    assert_tokens(
        "fn f():\n    return 1\n",
        &[
            "fn ident(f) ( ) : NEWLINE",
            "INDENT return int(1) NEWLINE",
            "DEDENT EOF",
        ],
    );
}

#[test]
fn block_then_top_level_statement() {
    assert_tokens(
        "if a:\n    b()\nc()\n",
        &[
            "if ident(a) : NEWLINE",
            "INDENT ident(b) ( ) NEWLINE",
            "DEDENT ident(c) ( ) NEWLINE",
            "EOF",
        ],
    );
}

#[test]
fn nested_blocks() {
    assert_tokens(
        "fn f():\n    while x:\n        y()\n    z()\n",
        &[
            "fn ident(f) ( ) : NEWLINE",
            "INDENT while ident(x) : NEWLINE",
            "INDENT ident(y) ( ) NEWLINE",
            "DEDENT ident(z) ( ) NEWLINE",
            "DEDENT EOF",
        ],
    );
}

#[test]
fn dedent_several_levels_at_once() {
    assert_tokens(
        "a:\n    b:\n        c:\n            d\ne\n",
        &[
            "ident(a) : NEWLINE",
            "INDENT ident(b) : NEWLINE",
            "INDENT ident(c) : NEWLINE",
            "INDENT ident(d) NEWLINE",
            "DEDENT DEDENT DEDENT ident(e) NEWLINE",
            "EOF",
        ],
    );
}

#[test]
fn dedent_two_of_three_levels() {
    assert_tokens(
        "a:\n    b:\n        c:\n            d\n    e\n",
        &[
            "ident(a) : NEWLINE",
            "INDENT ident(b) : NEWLINE",
            "INDENT ident(c) : NEWLINE",
            "INDENT ident(d) NEWLINE",
            "DEDENT DEDENT ident(e) NEWLINE",
            "DEDENT EOF",
        ],
    );
}

#[test]
fn all_blocks_closed_at_eof_without_newline() {
    assert_tokens(
        "a:\n    b:\n        c",
        &[
            "ident(a) : NEWLINE",
            "INDENT ident(b) : NEWLINE",
            "INDENT ident(c) NEWLINE",
            "DEDENT DEDENT EOF",
        ],
    );
}

#[test]
fn blank_lines_inside_block_are_ignored() {
    assert_tokens(
        "fn f():\n    a()\n\n\n    b()\n",
        &[
            "fn ident(f) ( ) : NEWLINE",
            "INDENT ident(a) ( ) NEWLINE",
            "ident(b) ( ) NEWLINE",
            "DEDENT EOF",
        ],
    );
}

#[test]
fn whitespace_only_lines_with_any_indentation_are_ignored() {
    assert_tokens(
        "fn f():\n    a()\n  \n          \n    b()\n",
        &[
            "fn ident(f) ( ) : NEWLINE",
            "INDENT ident(a) ( ) NEWLINE",
            "ident(b) ( ) NEWLINE",
            "DEDENT EOF",
        ],
    );
}

#[test]
fn comment_lines_at_any_indentation_are_ignored() {
    assert_tokens(
        "fn f():\n// left edge\n    a()\n          // way right\n  // misaligned\n    b()\n",
        &[
            "fn ident(f) ( ) : NEWLINE",
            "INDENT ident(a) ( ) NEWLINE",
            "ident(b) ( ) NEWLINE",
            "DEDENT EOF",
        ],
    );
}

#[test]
fn blank_line_between_blocks_does_not_dedent_early() {
    // The DEDENT comes at the next real line, not at the blank line.
    assert_tokens(
        "if a:\n    b\n\nelse:\n    c\n",
        &[
            "if ident(a) : NEWLINE",
            "INDENT ident(b) NEWLINE",
            "DEDENT else : NEWLINE",
            "INDENT ident(c) NEWLINE",
            "DEDENT EOF",
        ],
    );
}

#[test]
fn if_else_if_else_chain() {
    assert_tokens(
        "if a:\n    x\nelse if b:\n    y\nelse:\n    z\n",
        &[
            "if ident(a) : NEWLINE",
            "INDENT ident(x) NEWLINE",
            "DEDENT else if ident(b) : NEWLINE",
            "INDENT ident(y) NEWLINE",
            "DEDENT else : NEWLINE",
            "INDENT ident(z) NEWLINE",
            "DEDENT EOF",
        ],
    );
}

#[test]
fn reindent_after_dedent() {
    assert_tokens(
        "a:\n    b:\n        c\n    d:\n        e\n",
        &[
            "ident(a) : NEWLINE",
            "INDENT ident(b) : NEWLINE",
            "INDENT ident(c) NEWLINE",
            "DEDENT ident(d) : NEWLINE",
            "INDENT ident(e) NEWLINE",
            "DEDENT DEDENT EOF",
        ],
    );
}

#[test]
fn indent_and_dedent_spans() {
    let src = "a:\n    b\nc\n";
    let toks = lex(src).tokens;
    let indent = toks.iter().find(|t| t.kind == TokenKind::Indent).unwrap();
    assert_eq!(&src[indent.span.start..indent.span.end], "    ");
    let dedent = toks.iter().find(|t| t.kind == TokenKind::Dedent).unwrap();
    assert!(dedent.span.is_empty());
    assert_eq!(dedent.span.start, src.find('c').unwrap());
}

// ======================================================================
// Indentation errors
// ======================================================================

#[test]
fn tab_in_indentation_is_an_error() {
    let src = "fn f():\n\treturn 1\n";
    assert_errors(src, &["TDS0001"]);
    // Recovery: the tab counts as 4 spaces, so the block still opens.
    assert_eq!(
        dump(src),
        [
            "fn ident(f) ( ) : NEWLINE",
            "INDENT return int(1) NEWLINE",
            "DEDENT EOF"
        ]
    );
}

#[test]
fn tab_mixed_with_spaces_in_indentation() {
    assert_errors("fn f():\n  \treturn 1\n", &["TDS0001", "TDS0003"]);
}

#[test]
fn tab_between_tokens_is_an_error() {
    let src = "let x =\t5\n";
    assert_errors(src, &["TDS0001"]);
    assert_eq!(dump(src), ["let ident(x) = int(5) NEWLINE", "EOF"]);
}

#[test]
fn several_tabs_in_a_row_give_one_error() {
    assert_errors("let x =\t\t\t5\n", &["TDS0001"]);
}

#[test]
fn tabs_on_blank_lines_are_ignored() {
    assert_errors("a:\n    b\n\t\n    c\n", &[]);
}

#[test]
fn tabs_inside_strings_and_comments_are_fine() {
    assert_errors("let s = \"a\tb\" // c\td\n", &[]);
}

#[test]
fn misaligned_dedent() {
    let src = "a:\n    b:\n        c\n      d\n";
    assert_errors(src, &["TDS0002"]);
    // Recovery: `d` falls between two levels and stays in the deeper block.
    assert_eq!(
        dump(src),
        [
            "ident(a) : NEWLINE",
            "INDENT ident(b) : NEWLINE",
            "INDENT ident(c) NEWLINE",
            "ident(d) NEWLINE",
            "DEDENT DEDENT EOF",
        ]
    );
}

#[test]
fn misaligned_block_is_reported_once() {
    let src = "a:\n    b\n  c\n  d\ne\n";
    assert_errors(src, &["TDS0002"]);
    assert_eq!(
        dump(src),
        [
            "ident(a) : NEWLINE",
            "INDENT ident(b) NEWLINE",
            "ident(c) NEWLINE",
            "ident(d) NEWLINE",
            "DEDENT ident(e) NEWLINE",
            "EOF",
        ]
    );
}

#[test]
fn indent_of_two_spaces_is_an_error() {
    let src = "a:\n  b\n  c\n";
    // Only the first line of the block is reported; the second lines up.
    assert_errors(src, &["TDS0003"]);
    assert_eq!(
        dump(src),
        [
            "ident(a) : NEWLINE",
            "INDENT ident(b) NEWLINE",
            "ident(c) NEWLINE",
            "DEDENT EOF"
        ]
    );
}

#[test]
fn indent_of_eight_spaces_is_an_error() {
    assert_errors("a:\n        b\n", &["TDS0003"]);
}

#[test]
fn misaligned_line_followed_by_real_dedent() {
    let src = "a:\n    b:\n        c\n      d\n    e\nf\n";
    assert_errors(src, &["TDS0002"]);
    assert_eq!(
        dump(src),
        [
            "ident(a) : NEWLINE",
            "INDENT ident(b) : NEWLINE",
            "INDENT ident(c) NEWLINE",
            "ident(d) NEWLINE",
            "DEDENT ident(e) NEWLINE",
            "DEDENT ident(f) NEWLINE",
            "EOF",
        ]
    );
}

#[test]
fn indented_first_line_is_reported_as_bad_width() {
    assert_errors("  x\n", &["TDS0003"]);
}

#[test]
fn indentation_error_message_mentions_expected_column() {
    let err = &lex("a:\n  b\n").errors[0];
    assert!(err.help[0].contains("column 4"), "{:?}", err.help);
}

// ======================================================================
// Line joining: brackets and trailing operators
// ======================================================================

#[test]
fn newlines_inside_parens_are_ignored() {
    assert_tokens(
        "f(1,\n  2,\n        3)\n",
        &["ident(f) ( int(1) , int(2) , int(3) ) NEWLINE", "EOF"],
    );
}

#[test]
fn indentation_inside_brackets_is_ignored() {
    assert_tokens(
        "fn f():\n    let xs = [\n        1,\n  2,\n    ]\n    g()\n",
        &[
            "fn ident(f) ( ) : NEWLINE",
            "INDENT let ident(xs) = [ int(1) , int(2) , ] NEWLINE",
            "ident(g) ( ) NEWLINE",
            "DEDENT EOF",
        ],
    );
}

#[test]
fn blank_lines_and_comments_inside_brackets() {
    assert_tokens(
        "f(\n\n    // first\n    1,\n\n    2 // second\n)\n",
        &["ident(f) ( int(1) , int(2) ) NEWLINE", "EOF"],
    );
}

#[test]
fn nested_brackets_across_lines() {
    assert_tokens(
        "f([1,\n{2}],\n(3))\n",
        &[
            "ident(f) ( [ int(1) , { int(2) } ] , ( int(3) ) ) NEWLINE",
            "EOF",
        ],
    );
}

#[test]
fn trailing_binary_operator_continues_line() {
    assert_tokens(
        "let x = 1 +\n        2\nlet y = 3\n",
        &[
            "let ident(x) = int(1) + int(2) NEWLINE",
            "let ident(y) = int(3) NEWLINE",
            "EOF",
        ],
    );
}

#[test]
fn trailing_keyword_operator_continues_line() {
    assert_tokens(
        "if a and\n  b:\n    c\n",
        &[
            "if ident(a) and ident(b) : NEWLINE",
            "INDENT ident(c) NEWLINE",
            "DEDENT EOF",
        ],
    );
}

#[test]
fn trailing_comma_continues_line() {
    assert_tokens(
        "let a, \n b = 1\n",
        &["let ident(a) , ident(b) = int(1) NEWLINE", "EOF"],
    );
}

#[test]
fn trailing_equals_continues_line() {
    assert_tokens(
        "fn square(x: Int) -> Int =\n    x * x\n",
        &["fn ident(square) ( ident(x) : ident(Int) ) -> ident(Int) = ident(x) * ident(x) NEWLINE", "EOF"],
    );
}

#[test]
fn continuation_line_may_be_followed_by_blank_line() {
    assert_tokens(
        "let x = 1 +\n\n    2\n",
        &["let ident(x) = int(1) + int(2) NEWLINE", "EOF"],
    );
}

#[test]
fn continuation_inside_block_keeps_block_level() {
    assert_tokens(
        "fn f():\n    let x = 1 +\n2\n    g()\n",
        &[
            "fn ident(f) ( ) : NEWLINE",
            "INDENT let ident(x) = int(1) + int(2) NEWLINE",
            "ident(g) ( ) NEWLINE",
            "DEDENT EOF",
        ],
    );
}

#[test]
fn trailing_colon_does_not_continue_line() {
    assert_tokens(
        "while x:\n    y\n",
        &[
            "while ident(x) : NEWLINE",
            "INDENT ident(y) NEWLINE",
            "DEDENT EOF",
        ],
    );
}

#[test]
fn trailing_operator_followed_by_comment_still_continues() {
    assert_tokens(
        "let x = 1 + // more\n    2\n",
        &["let ident(x) = int(1) + int(2) NEWLINE", "EOF"],
    );
}

// ======================================================================
// Bracket errors
// ======================================================================

#[test]
fn unmatched_closing_bracket() {
    assert_errors("f())\n", &["TDS0008"]);
}

#[test]
fn mismatched_closing_bracket() {
    let out = lex("f(a]\n");
    assert_eq!(
        out.errors.iter().map(|e| e.code).collect::<Vec<_>>(),
        ["TDS0009"]
    );
    // Points back at the `(` it should have closed.
    assert_eq!(out.errors[0].labels.len(), 2);
}

#[test]
fn closing_outer_bracket_reports_inner_once() {
    // `]` is missing; `)` closes the call and the `[` is dropped.
    assert_errors("f([1, 2)\nx\n", &["TDS0009"]);
}

#[test]
fn unclosed_bracket_at_eof() {
    let src = "f(1,\n";
    assert_errors(src, &["TDS0010"]);
    // A closing `)` is synthesized so the parser sees balanced brackets.
    assert_eq!(dump(src), ["ident(f) ( int(1) , ) NEWLINE", "EOF"]);
}

#[test]
fn unclosed_bracket_inside_block_still_closes_blocks() {
    let src = "fn f():\n    g(\n";
    assert_errors(src, &["TDS0010"]);
    assert_eq!(
        dump(src),
        [
            "fn ident(f) ( ) : NEWLINE",
            "INDENT ident(g) ( ) NEWLINE",
            "DEDENT EOF"
        ]
    );
}

// ======================================================================
// Identifiers and keywords
// ======================================================================

#[test]
fn keywords() {
    assert_tokens(
        "let var fn return if else while for in break continue struct enum match impl interface",
        &["let var fn return if else while for in break continue struct enum match impl interface NEWLINE", "EOF"],
    );
    assert_tokens(
        "true false and or not mut take ref pub use none self unsafe const static extern _",
        &["true false and or not mut take ref pub use none self unsafe const static extern _ NEWLINE", "EOF"],
    );
}

#[test]
fn identifiers() {
    assert_tokens(
        "x _y snake_case Camel42 lets read Self __",
        &["ident(x) ident(_y) ident(snake_case) ident(Camel42) ident(lets) ident(read) ident(Self) ident(__) NEWLINE", "EOF"],
    );
}

#[test]
fn unicode_identifiers() {
    assert_tokens("let café = 1", &["let ident(café) = int(1) NEWLINE", "EOF"]);
}

// ======================================================================
// Numbers
// ======================================================================

#[test]
fn integers() {
    assert_tokens(
        "0 42 1_000_000 0xFF 0x4002_0000 0b1010 0o17",
        &[
            "int(0) int(42) int(1000000) int(255) int(1073872896) int(10) int(15) NEWLINE",
            "EOF",
        ],
    );
}

#[test]
fn floats() {
    assert_tokens(
        "3.14 0.5 1e10 1.5e-3 2E+2 1_000.5",
        &["float(3.14) float(0.5) float(10000000000.0) float(0.0015) float(200.0) float(1000.5) NEWLINE", "EOF"],
    );
}

#[test]
fn ranges_are_not_floats() {
    assert_tokens(
        "0..<10 1...10 a..b",
        &[
            "int(0) ..< int(10) int(1) ... int(10) ident(a) .. ident(b) NEWLINE",
            "EOF",
        ],
    );
}

#[test]
fn method_call_on_integer() {
    assert_tokens("5.abs()", &["int(5) . ident(abs) ( ) NEWLINE", "EOF"]);
}

#[test]
fn max_u64_literal_is_fine() {
    assert_tokens(
        "18446744073709551615",
        &["int(18446744073709551615) NEWLINE", "EOF"],
    );
}

#[test]
fn integer_too_large() {
    assert_errors("18446744073709551616", &["TDS0011"]);
    assert_errors("0xFFFFFFFFFFFFFFFFF", &["TDS0011"]);
}

#[test]
fn invalid_number_suffix() {
    let src = "12abc + 1";
    assert_errors(src, &["TDS0012"]);
    assert_eq!(dump(src), ["ERROR + int(1) NEWLINE", "EOF"]);
}

#[test]
fn prefix_without_digits() {
    assert_errors("0x", &["TDS0012"]);
    assert_errors("0b102", &["TDS0012"]);
}

#[test]
fn exponent_without_digits() {
    assert_errors("1e", &["TDS0012"]);
    assert_errors("1.5e+", &["TDS0012"]);
}

// ======================================================================
// Operators and punctuation
// ======================================================================

#[test]
fn operators() {
    assert_tokens(
        "+ - * / % == != < <= > >= = += -= *= /= %=",
        &["+ - * / % == != < <= > >= = += -= *= /= %= NEWLINE", "EOF"],
    );
}

#[test]
fn punctuation() {
    assert_tokens(
        "-> ? ?? ! @ . .. ..< ... , : & | ^ ~ << >>",
        &["-> ? ?? ! @ . .. ..< ... , : & | ^ ~ << >> NEWLINE", "EOF"],
    );
}

#[test]
fn operators_without_spaces() {
    assert_tokens(
        "a+=b*-c",
        &["ident(a) += ident(b) * - ident(c) NEWLINE", "EOF"],
    );
    assert_tokens(
        "x!.y??z",
        &["ident(x) ! . ident(y) ?? ident(z) NEWLINE", "EOF"],
    );
    assert_tokens("a<-b", &["ident(a) < - ident(b) NEWLINE", "EOF"]);
}

#[test]
fn generics_use_square_brackets() {
    assert_tokens(
        "fn largest[T: Ordered](items: List[T]) -> T?:",
        &["fn ident(largest) [ ident(T) : ident(Ordered) ] ( ident(items) : ident(List) [ ident(T) ] ) -> ident(T) ? : NEWLINE", "EOF"],
    );
}

#[test]
fn attribute() {
    assert_tokens(
        "@derive(Equal)",
        &["@ ident(derive) ( ident(Equal) ) NEWLINE", "EOF"],
    );
}

#[test]
fn unexpected_character() {
    let src = "let x = $5\n";
    assert_errors(src, &["TDS0004"]);
    assert_eq!(dump(src), ["let ident(x) = ERROR int(5) NEWLINE", "EOF"]);
}

#[test]
fn semicolon_gets_a_helpful_hint() {
    let out = lex("let x = 5;");
    assert_eq!(out.errors[0].code, "TDS0004");
    assert!(out.errors[0].help[0].contains("semicolons"));
}

// ======================================================================
// Strings
// ======================================================================

#[test]
fn plain_strings() {
    assert_tokens(r#""hello" """#, &[r#"str("hello") str() NEWLINE"#, "EOF"]);
}

#[test]
fn string_escapes() {
    assert_tokens(
        r#""a\nb\t\"q\" \\ \{x\} \u{41}\u{1F600}\0""#,
        &[r#"str("a\nb\t\"q\" \\ {x} A😀\0") NEWLINE"#, "EOF"],
    );
}

#[test]
fn lone_closing_brace_in_string_is_literal() {
    assert_tokens(r#""a}b""#, &[r#"str("a}b") NEWLINE"#, "EOF"]);
}

#[test]
fn interpolation() {
    assert_tokens(
        r#""Hello, {name}!""#,
        &[r#"str("Hello, " {ident(name)} "!") NEWLINE"#, "EOF"],
    );
}

#[test]
fn interpolation_only() {
    assert_tokens(r#""{x}""#, &["str({ident(x)}) NEWLINE", "EOF"]);
}

#[test]
fn adjacent_interpolations() {
    assert_tokens(
        r#""{a}{b}""#,
        &["str({ident(a)} {ident(b)}) NEWLINE", "EOF"],
    );
}

#[test]
fn interpolation_with_expression() {
    assert_tokens(
        r#""sum = {a + b * 2}, first = {xs[0]}""#,
        &[
            r#"str("sum = " {ident(a) + ident(b) * int(2)} ", first = " {ident(xs) [ int(0) ]}) NEWLINE"#,
            "EOF",
        ],
    );
}

#[test]
fn interpolation_with_nested_string() {
    assert_tokens(
        r#""{greet("Bob")}!""#,
        &[r#"str({ident(greet) ( str("Bob") )} "!") NEWLINE"#, "EOF"],
    );
}

#[test]
fn interpolation_with_nested_interpolated_string() {
    assert_tokens(
        r#""{f("x{y}z")}""#,
        &[
            r#"str({ident(f) ( str("x" {ident(y)} "z") )}) NEWLINE"#,
            "EOF",
        ],
    );
}

#[test]
fn interpolation_with_nested_braces() {
    assert_tokens(r#""{ {a} }""#, &["str({{ ident(a) }}) NEWLINE", "EOF"]);
}

#[test]
fn interpolation_token_spans_are_absolute() {
    let src = r#"print("hi {name}")"#;
    let toks = lex(src).tokens;
    let TokenKind::Str(parts) = &toks[2].kind else {
        panic!("expected a string, got {:?}", toks[2].kind)
    };
    let StrPart::Interp { tokens, span } = &parts[1] else {
        panic!("expected an interpolation")
    };
    assert_eq!(&src[span.start..span.end], "name");
    assert_eq!(&src[tokens[0].span.start..tokens[0].span.end], "name");
    assert_eq!(&src[toks[2].span.start..toks[2].span.end], r#""hi {name}""#);
}

#[test]
fn unterminated_string() {
    let src = "let s = \"abc\nlet t = 1\n";
    assert_errors(src, &["TDS0005"]);
    // The next line still lexes normally.
    assert_eq!(
        dump(src),
        [
            r#"let ident(s) = str("abc") NEWLINE"#,
            "let ident(t) = int(1) NEWLINE",
            "EOF"
        ]
    );
}

#[test]
fn unterminated_string_at_eof() {
    assert_errors("\"abc", &["TDS0005"]);
}

#[test]
fn unknown_escape() {
    let src = r#""a\qb""#;
    assert_errors(src, &["TDS0006"]);
    assert_eq!(dump(src), [r#"str("ab") NEWLINE"#, "EOF"]);
}

#[test]
fn bad_unicode_escapes() {
    assert_errors(r#""\u41""#, &["TDS0006"]);
    assert_errors(r#""\u{}""#, &["TDS0006"]);
    assert_errors(r#""\u{D800}""#, &["TDS0006"]);
    assert_errors(r#""\u{1234567}""#, &["TDS0006"]);
}

#[test]
fn unterminated_interpolation() {
    let src = "let s = \"a {b\nlet t = 1\n";
    // Reported once, not also as an unterminated string.
    assert_errors(src, &["TDS0007"]);
    assert_eq!(
        dump(src),
        [
            r#"let ident(s) = str("a ") NEWLINE"#,
            "let ident(t) = int(1) NEWLINE",
            "EOF"
        ]
    );
}

#[test]
fn empty_interpolation() {
    assert_errors(r#""a{}b""#, &["TDS0013"]);
    assert_errors(r#""a{  }b""#, &["TDS0013"]);
}

#[test]
fn errors_inside_interpolation_are_reported() {
    assert_errors(r#""{a $ b}""#, &["TDS0004"]);
    assert_errors(r#""{f(a}""#, &["TDS0010"]);
}

// ======================================================================
// Chars and lifetimes
// ======================================================================

#[test]
fn char_literals() {
    assert_tokens(
        r"'a' '\n' '\'' 'é'",
        &["char('a') char('\\n') char('\\'') char('é') NEWLINE", "EOF"],
    );
}

#[test]
fn lifetimes() {
    assert_tokens(
        "fn first_word['a](text: ref['a] Str)",
        &["fn ident(first_word) [ lifetime('a) ] ( ident(text) : ref [ lifetime('a) ] ident(Str) ) NEWLINE", "EOF"],
    );
}

#[test]
fn invalid_char_literals() {
    assert_errors("''", &["TDS0014"]);
    assert_errors("'ab'", &["TDS0014"]);
    assert_errors("'1", &["TDS0014"]);
}

// ======================================================================
// Whole programs
// ======================================================================

/// The syntax tour from the language brief must lex without errors and
/// with balanced blocks.
#[test]
fn syntax_tour_lexes_cleanly() {
    let src = include_str!("data/tour.tds");
    let out = lex(src);
    assert!(
        out.errors.is_empty(),
        "unexpected errors: {:#?}",
        out.errors
    );
    let count = |k: TokenKind| out.tokens.iter().filter(|t| t.kind == k).count();
    assert!(count(TokenKind::Indent) > 10);
    assert_eq!(count(TokenKind::Indent), count(TokenKind::Dedent));
    assert_eq!(out.tokens.last().unwrap().kind, TokenKind::Eof);
}

#[test]
fn fizzbuzz() {
    let src = "\
fn main():
    for i in 1...100:
        if i % 15 == 0:
            print(\"FizzBuzz\")
        else if i % 3 == 0:
            print(\"Fizz\")
        else:
            print(i)
";
    assert_tokens(
        src,
        &[
            "fn ident(main) ( ) : NEWLINE",
            "INDENT for ident(i) in int(1) ... int(100) : NEWLINE",
            "INDENT if ident(i) % int(15) == int(0) : NEWLINE",
            r#"INDENT ident(print) ( str("FizzBuzz") ) NEWLINE"#,
            "DEDENT else if ident(i) % int(3) == int(0) : NEWLINE",
            r#"INDENT ident(print) ( str("Fizz") ) NEWLINE"#,
            "DEDENT else : NEWLINE",
            "INDENT ident(print) ( ident(i) ) NEWLINE",
            "DEDENT DEDENT DEDENT EOF",
        ],
    );
}

#[test]
fn every_token_span_is_within_source() {
    let src = include_str!("data/tour.tds");
    for tok in lex(src).tokens {
        assert!(
            tok.span.start <= tok.span.end && tok.span.end <= src.len(),
            "{tok:?}"
        );
    }
}

#[test]
fn many_errors_do_not_stop_the_lexer() {
    let src = "a:\n\tb $ c\n  d)\n\"open\n";
    let out = lex(src);
    assert!(out.errors.len() >= 4, "{:#?}", out.errors);
    assert_eq!(out.tokens.last().unwrap().kind, TokenKind::Eof);
}
