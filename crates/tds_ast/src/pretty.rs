//! A readable dump of the AST, used by parser snapshot tests and by
//! `tds parse` for debugging.
//!
//! Items and statements go one per line, indented by nesting. Expressions
//! are printed as S-expressions, so precedence is explicit:
//! `1 + 2 * 3` prints as `(+ 1 (* 2 3))`. An `if` or `match` in statement
//! position is printed over several lines.

use std::fmt::Write;

use crate::*;

pub fn dump_module(module: &Module) -> String {
    let mut p = Printer::default();
    for item in &module.items {
        p.item(item, 0);
    }
    p.out
}

pub fn dump_repl(entries: &[TopLevel]) -> String {
    let mut p = Printer::default();
    for entry in entries {
        match entry {
            TopLevel::Item(item) => p.item(item, 0),
            TopLevel::Stmt(stmt) => p.stmt(stmt, 0),
        }
    }
    p.out
}

#[derive(Default)]
struct Printer {
    out: String,
}

impl Printer {
    fn line(&mut self, indent: usize, text: &str) {
        for _ in 0..indent {
            self.out.push_str("  ");
        }
        self.out.push_str(text);
        self.out.push('\n');
    }

    fn item(&mut self, item: &Item, indent: usize) {
        match item {
            Item::Fn(f) => {
                let params: Vec<String> = f
                    .params
                    .iter()
                    .map(|p| match p.mode {
                        Mode::Read => format!("{}: {}", p.name.name, ty(&p.ty)),
                        mode => format!("{}: {} {}", p.name.name, mode.keyword(), ty(&p.ty)),
                    })
                    .collect();
                let ret = f.ret.as_ref().map_or("()".to_string(), ty);
                self.line(
                    indent,
                    &format!("fn {}({}) -> {}", f.name.name, params.join(", "), ret),
                );
                self.block(&f.body, indent + 1);
            }
            Item::Struct(s) => {
                self.line(indent, &format!("struct {}", s.name.name));
                for field in &s.fields {
                    let mut text = format!("{}: {}", field.name.name, ty(&field.ty));
                    if let Some(default) = &field.default {
                        write!(text, " = {}", expr(default)).unwrap();
                    }
                    self.line(indent + 1, &text);
                }
            }
            Item::Enum(e) => {
                self.line(indent, &format!("enum {}", e.name.name));
                for variant in &e.variants {
                    if variant.fields.is_empty() {
                        self.line(indent + 1, &variant.name.name);
                    } else {
                        let fields: Vec<String> = variant
                            .fields
                            .iter()
                            .map(|f| match &f.name {
                                Some(name) => format!("{}: {}", name.name, ty(&f.ty)),
                                None => ty(&f.ty),
                            })
                            .collect();
                        self.line(
                            indent + 1,
                            &format!("{}({})", variant.name.name, fields.join(", ")),
                        );
                    }
                }
            }
        }
    }

    fn block(&mut self, block: &Block, indent: usize) {
        for stmt in &block.stmts {
            self.stmt(stmt, indent);
        }
    }

    fn stmt(&mut self, stmt: &Stmt, indent: usize) {
        match &stmt.kind {
            StmtKind::Let {
                mutable,
                name,
                ty: annotation,
                init,
            } => {
                let keyword = if *mutable { "var" } else { "let" };
                let mut head = format!("{keyword} {}", name.name);
                if let Some(t) = annotation {
                    write!(head, ": {}", ty(t)).unwrap();
                }
                self.expr_stmt(&format!("{head} = "), init, indent);
            }
            StmtKind::Assign { target, op, value } => {
                self.expr_stmt(&format!("{} {} ", expr(target), op.symbol()), value, indent);
            }
            StmtKind::Expr(e) => self.expr_stmt("", e, indent),
            StmtKind::While { cond, body } => {
                self.line(indent, &format!("while {}", expr(cond)));
                self.block(body, indent + 1);
            }
            StmtKind::For { var, iter, body } => {
                self.line(indent, &format!("for {} in {}", var.name, expr(iter)));
                self.block(body, indent + 1);
            }
            StmtKind::Return(None) => self.line(indent, "return"),
            StmtKind::Return(Some(e)) => self.expr_stmt("return ", e, indent),
            StmtKind::Break => self.line(indent, "break"),
            StmtKind::Continue => self.line(indent, "continue"),
        }
    }

    /// Prints `prefix` followed by an expression; `if` and `match` get the
    /// multi-line layout.
    fn expr_stmt(&mut self, prefix: &str, e: &Expr, indent: usize) {
        match &e.kind {
            ExprKind::If { .. } => self.if_expr(prefix, e, indent),
            ExprKind::Match { scrutinee, arms } => {
                self.line(indent, &format!("{prefix}match {}", expr(scrutinee)));
                for arm in arms {
                    self.line(indent + 1, &format!("case {}", pattern(&arm.pattern)));
                    self.block(&arm.body, indent + 2);
                }
            }
            _ => self.line(indent, &format!("{prefix}{}", expr(e))),
        }
    }

    fn if_expr(&mut self, prefix: &str, e: &Expr, indent: usize) {
        let ExprKind::If { cond, then, else_ } = &e.kind else {
            unreachable!()
        };
        self.line(indent, &format!("{prefix}if {}", expr(cond)));
        self.block(then, indent + 1);
        match else_.as_deref() {
            None => {}
            Some(
                else_expr @ Expr {
                    kind: ExprKind::If { .. },
                    ..
                },
            ) => {
                self.if_expr("else ", else_expr, indent);
            }
            Some(Expr {
                kind: ExprKind::Block(block),
                ..
            }) => {
                self.line(indent, "else");
                self.block(block, indent + 1);
            }
            Some(other) => {
                self.line(indent, "else");
                self.line(indent + 1, &expr(other));
            }
        }
    }
}

fn ty(t: &TypeExpr) -> String {
    match &t.kind {
        TypeExprKind::Named { name, args } if args.is_empty() => name.name.clone(),
        TypeExprKind::Named { name, args } => {
            let args: Vec<String> = args.iter().map(ty).collect();
            format!("{}[{}]", name.name, args.join(", "))
        }
        TypeExprKind::Unit => "()".to_string(),
        TypeExprKind::Optional(inner) => format!("{}?", ty(inner)),
    }
}

fn pattern(p: &Pattern) -> String {
    match &p.kind {
        PatternKind::Wildcard => "_".to_string(),
        PatternKind::Name(name) => name.clone(),
        PatternKind::Variant { name, args } => {
            let args: Vec<String> = args.iter().map(pattern).collect();
            format!("{}({})", name.name, args.join(", "))
        }
        PatternKind::Int(n) => n.to_string(),
        PatternKind::Str(s) => format!("{s:?}"),
        PatternKind::Bool(b) => b.to_string(),
        PatternKind::Char(c) => format!("{c:?}"),
    }
}

/// An expression as an S-expression on one line.
fn expr(e: &Expr) -> String {
    match &e.kind {
        ExprKind::Int(n) => n.to_string(),
        ExprKind::Float(x) => format!("{x:?}"),
        ExprKind::Bool(b) => b.to_string(),
        ExprKind::Char(c) => format!("{c:?}"),
        ExprKind::Str(segments) => match segments.as_slice() {
            [] => "\"\"".to_string(),
            [StrSegment::Text(text)] => format!("{text:?}"),
            _ => {
                let parts: Vec<String> = segments
                    .iter()
                    .map(|s| match s {
                        StrSegment::Text(text) => format!("{text:?}"),
                        StrSegment::Expr(e) => format!("{{{}}}", expr(e)),
                    })
                    .collect();
                format!("(str {})", parts.join(" "))
            }
        },
        ExprKind::None => "none".to_string(),
        ExprKind::Unit => "()".to_string(),
        ExprKind::Name(name) => name.clone(),
        ExprKind::List(items) => sexpr("list", items.iter().map(expr)),
        ExprKind::Unary { op, expr: inner } => format!("({} {})", op.symbol(), expr(inner)),
        ExprKind::Binary { op, lhs, rhs } => {
            format!("({} {} {})", op.symbol(), expr(lhs), expr(rhs))
        }
        ExprKind::Range {
            start,
            end,
            inclusive,
        } => {
            let op = if *inclusive { "..." } else { "..<" };
            format!("({op} {} {})", expr(start), expr(end))
        }
        ExprKind::Call { callee, args } => sexpr(
            "call",
            std::iter::once(expr(callee)).chain(args.iter().map(arg)),
        ),
        ExprKind::MethodCall {
            receiver,
            method,
            args,
        } => sexpr(
            &format!("method {}", method.name),
            std::iter::once(expr(receiver)).chain(args.iter().map(arg)),
        ),
        ExprKind::Field { expr: inner, field } => format!("(. {} {})", expr(inner), field.name),
        ExprKind::Index { expr: inner, index } => {
            format!("(index {} {})", expr(inner), expr(index))
        }
        ExprKind::Unwrap(inner) => format!("(! {})", expr(inner)),
        ExprKind::Take(inner) => format!("(take {})", expr(inner)),
        ExprKind::If { cond, then, else_ } => {
            let mut text = format!("(if {} {}", expr(cond), inline_block(then));
            if let Some(else_) = else_ {
                write!(text, " {}", expr(else_)).unwrap();
            }
            text.push(')');
            text
        }
        ExprKind::Match { scrutinee, arms } => {
            let arms = arms
                .iter()
                .map(|arm| format!("({} {})", pattern(&arm.pattern), inline_block(&arm.body)));
            sexpr("match", std::iter::once(expr(scrutinee)).chain(arms))
        }
        ExprKind::Block(block) => inline_block(block),
    }
}

fn arg(a: &Arg) -> String {
    let mut text = String::new();
    if let Some(label) = &a.label {
        write!(text, "{}: ", label.name).unwrap();
    }
    match a.mode {
        Mode::Read => text.push_str(&expr(&a.value)),
        mode => write!(text, "({} {})", mode.keyword(), expr(&a.value)).unwrap(),
    }
    if a.label.is_some() {
        format!("({text})")
    } else {
        text
    }
}

/// A block on one line: a single expression prints bare, anything else
/// as `(block stmt; stmt)`.
fn inline_block(block: &Block) -> String {
    if let [Stmt {
        kind: StmtKind::Expr(e),
        ..
    }] = block.stmts.as_slice()
    {
        return expr(e);
    }
    let mut p = Printer::default();
    for stmt in &block.stmts {
        p.stmt(stmt, 0);
    }
    let stmts: Vec<&str> = p.out.lines().map(str::trim).collect();
    format!("(block {})", stmts.join("; "))
}

fn sexpr(head: &str, parts: impl Iterator<Item = String>) -> String {
    let mut text = format!("({head}");
    for part in parts {
        text.push(' ');
        text.push_str(&part);
    }
    text.push(')');
    text
}
