//! Module resolution tests, using in-memory files.

use tds_ast::dump_module;
use tds_resolve::{load_program, MemoryLoader};

const SHAPES: &str = "\
pub enum Shape:
    Circle(radius: Float)
    Point

pub fn area(s: Shape) -> Float:
    match s:
        Circle(r): 3.0 * r * r
        Point: 0.0

fn helper() -> Int = 1

pub struct Motor:
    name: String
";

fn load(main: &str, files: &[(&str, &str)]) -> (String, Vec<&'static str>) {
    let loader = MemoryLoader(
        files
            .iter()
            .map(|(p, t)| (p.to_string(), t.to_string()))
            .collect(),
    );
    let (program, mut diags) = load_program("main.tds", main, &loader);
    if diags.is_empty() {
        diags = tds_check::check_module(&program.module);
    }
    (
        dump_module(&program.module),
        diags.iter().map(|d| d.code).collect(),
    )
}

#[track_caller]
fn ok(main: &str, files: &[(&str, &str)]) -> String {
    let (dump, codes) = load(main, files);
    assert_eq!(codes, Vec::<&str>::new(), "\n{dump}");
    dump
}

#[track_caller]
fn errors(main: &str, files: &[(&str, &str)], expected: &[&str]) {
    assert_eq!(load(main, files).1, expected);
}

#[test]
fn single_file_is_unchanged() {
    let dump = ok("fn main():\n    print(1)\n", &[]);
    assert_eq!(dump, "fn main() -> ()\n  (call print 1)\n");
}

#[test]
fn module_import_uses_qualified_names() {
    let dump = ok(
        "use geo.shapes\n\nfn main():\n    let s = shapes.Shape.Circle(1.0)\n    print(shapes.area(s))\n    let p: shapes.Shape = shapes.Shape.Point\n",
        &[("geo/shapes.tds", SHAPES)],
    );
    assert!(dump.contains("(call geo.shapes.area s)"), "{dump}");
    assert!(
        dump.contains("(method geo.shapes.Circle geo.shapes.Shape 1.0)"),
        "{dump}"
    );
    assert!(
        dump.contains("let p: geo.shapes.Shape = (. geo.shapes.Shape geo.shapes.Point)"),
        "{dump}"
    );
    // The module's own items are renamed, and its references too.
    assert!(
        dump.contains("fn geo.shapes.area(s: geo.shapes.Shape) -> Float"),
        "{dump}"
    );
    assert!(dump.contains("case geo.shapes.Circle(r)"), "{dump}");
}

#[test]
fn item_imports_bring_enum_variants() {
    let dump = ok(
        "use geo.shapes.{Shape, area}\n\nfn main():\n    print(area(Circle(radius: 2.0)))\n    match Point:\n        Circle(_): print(1)\n        Point: print(2)\n",
        &[("geo/shapes.tds", SHAPES)],
    );
    assert!(
        dump.contains("(call geo.shapes.area (call geo.shapes.Circle (radius: 2.0)))"),
        "{dump}"
    );
}

#[test]
fn single_item_import() {
    ok(
        "use geo.shapes.Motor\n\nfn main():\n    let m = Motor(name: \"m\")\n    print(m.name)\n",
        &[("geo/shapes.tds", SHAPES)],
    );
}

#[test]
fn locals_shadow_imported_names() {
    let dump = ok(
        "use geo.shapes.{area}\n\nfn main():\n    let area = 5\n    print(area)\n",
        &[("geo/shapes.tds", SHAPES)],
    );
    assert!(dump.contains("(call print area)"), "{dump}");
}

#[test]
fn modules_can_import_each_other() {
    ok(
        "use a\n\nfn main():\n    print(a.f())\n",
        &[
            ("a.tds", "use b\n\npub fn f() -> Int = b.g() + 1\n"),
            (
                "b.tds",
                "use a\n\npub fn g() -> Int = 41\n\npub fn h() -> Int = a.f()\n",
            ),
        ],
    );
}

#[test]
fn same_names_in_different_modules_dont_clash() {
    ok(
        "use a\nuse b\n\nfn main():\n    print(a.Kind.Point == a.Kind.Point)\n    print(b.Kind.Point)\n",
        &[
            ("a.tds", "pub enum Kind:\n    Point\n"),
            ("b.tds", "pub enum Kind:\n    Point\n    Line\n"),
        ],
    );
}

#[test]
fn unknown_module() {
    errors(
        "use nowhere\n\nfn main():\n    print(1)\n",
        &[],
        &["TDS0240"],
    );
}

#[test]
fn private_item() {
    errors(
        "use geo.shapes.{helper}\n\nfn main():\n    print(helper())\n",
        &[("geo/shapes.tds", SHAPES)],
        &["TDS0241"],
    );
    errors(
        "use geo.shapes\n\nfn main():\n    print(shapes.helper())\n",
        &[("geo/shapes.tds", SHAPES)],
        &["TDS0241"],
    );
}

#[test]
fn unknown_import() {
    errors(
        "use geo.shapes.{Nope}\n\nfn main():\n    print(1)\n",
        &[("geo/shapes.tds", SHAPES)],
        &["TDS0242"],
    );
}

#[test]
fn import_conflict() {
    errors(
        "use geo.shapes.{area}\n\nfn area() -> Int = 1\n\nfn main():\n    print(1)\n",
        &[("geo/shapes.tds", SHAPES)],
        &["TDS0243"],
    );
}

#[test]
fn module_used_as_value() {
    errors(
        "use geo.shapes\n\nfn main():\n    print(shapes)\n",
        &[("geo/shapes.tds", SHAPES)],
        &["TDS0244"],
    );
}

#[test]
fn standard_library_is_not_available_yet() {
    errors(
        "use std.io\n\nfn main():\n    print(1)\n",
        &[],
        &["TDS0106"],
    );
}

#[test]
fn errors_in_modules_point_at_the_module_file() {
    let loader = MemoryLoader(vec![(
        "m.tds".to_string(),
        "pub fn f( -> Int = 1\n".to_string(),
    )]);
    let (program, diags) = load_program("main.tds", "use m\n\nfn main():\n    print(1)\n", &loader);
    assert_eq!(diags.len(), 1);
    let span = diags[0].primary_span().unwrap();
    assert_eq!(program.sources.file(span.start).unwrap().name, "m.tds");
}
