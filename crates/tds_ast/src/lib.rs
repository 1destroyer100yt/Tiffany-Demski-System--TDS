//! The TDS abstract syntax tree.
//!
//! The AST is a faithful picture of what the user wrote: no names are
//! resolved and nothing is desugared yet. Every node carries a [`Span`] so
//! later stages can point errors at the source.
//!
//! In Phase 1 the type checker and the interpreter both work on this tree.
//! From Phase 2 on, name resolution lowers it to HIR, and the interpreter
//! moves to TIR.

mod pretty;

pub use pretty::{dump_module, dump_repl};

use tds_diagnostics::Span;

/// A name as written, with its location.
#[derive(Debug, Clone, PartialEq)]
pub struct Ident {
    pub name: String,
    pub span: Span,
}

// ======================================================================
// Top level
// ======================================================================

/// A whole source file: a list of items.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Module {
    pub items: Vec<Item>,
}

/// One entry typed at the REPL: items and statements may be mixed.
#[derive(Debug, Clone, PartialEq)]
pub enum TopLevel {
    Item(Item),
    Stmt(Stmt),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Fn(FnDecl),
    Struct(StructDecl),
    Enum(EnumDecl),
}

impl Item {
    pub fn name(&self) -> &Ident {
        match self {
            Item::Fn(f) => &f.name,
            Item::Struct(s) => &s.name,
            Item::Enum(e) => &e.name,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FnDecl {
    pub name: Ident,
    pub params: Vec<Param>,
    /// `None` means the function returns `()`.
    pub ret: Option<TypeExpr>,
    /// For the `fn f() = expr` form this is a block holding one expression.
    pub body: Block,
    pub span: Span,
}

/// How a parameter receives its argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Shared read-only borrow (the default, never written).
    Read,
    /// Exclusive borrow; the caller sees changes. Written `mut`.
    Mut,
    /// Ownership moves in. Written `take`.
    Take,
}

impl Mode {
    pub fn keyword(self) -> &'static str {
        match self {
            Mode::Read => "read",
            Mode::Mut => "mut",
            Mode::Take => "take",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub name: Ident,
    pub mode: Mode,
    pub ty: TypeExpr,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StructDecl {
    pub name: Ident,
    pub fields: Vec<FieldDecl>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FieldDecl {
    pub name: Ident,
    pub ty: TypeExpr,
    pub default: Option<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnumDecl {
    pub name: Ident,
    pub variants: Vec<VariantDecl>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VariantDecl {
    pub name: Ident,
    /// Payload fields. Empty for a unit variant like `Point`.
    pub fields: Vec<VariantField>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VariantField {
    /// `Circle(radius: Float)` has a name; `Circle(Float)` doesn't.
    pub name: Option<Ident>,
    pub ty: TypeExpr,
}

// ======================================================================
// Types
// ======================================================================

#[derive(Debug, Clone, PartialEq)]
pub struct TypeExpr {
    pub kind: TypeExprKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TypeExprKind {
    /// `Int`, `List[Int]`, `Shape`.
    Named { name: Ident, args: Vec<TypeExpr> },
    /// `()`
    Unit,
    /// `T?`
    Optional(Box<TypeExpr>),
}

// ======================================================================
// Statements
// ======================================================================

/// An indented block of statements. When a block is used as a value (a
/// function body, an `if` branch, a match arm), its value is the value of
/// its last statement if that is an expression.
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Stmt {
    pub kind: StmtKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StmtKind {
    /// `let x = e` / `var x: T = e`
    Let {
        mutable: bool,
        name: Ident,
        ty: Option<TypeExpr>,
        init: Expr,
    },
    /// `target = value` or a compound form like `target += value`.
    Assign {
        target: Expr,
        op: AssignOp,
        value: Expr,
    },
    Expr(Expr),
    While {
        cond: Expr,
        body: Block,
    },
    For {
        var: Ident,
        iter: Expr,
        body: Block,
    },
    Return(Option<Expr>),
    Break,
    Continue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssignOp {
    Assign,
    Add,
    Sub,
    Mul,
    Div,
    Rem,
}

impl AssignOp {
    pub fn symbol(self) -> &'static str {
        match self {
            AssignOp::Assign => "=",
            AssignOp::Add => "+=",
            AssignOp::Sub => "-=",
            AssignOp::Mul => "*=",
            AssignOp::Div => "/=",
            AssignOp::Rem => "%=",
        }
    }

    /// The arithmetic operator behind a compound assignment.
    pub fn binary_op(self) -> Option<BinaryOp> {
        match self {
            AssignOp::Assign => None,
            AssignOp::Add => Some(BinaryOp::Add),
            AssignOp::Sub => Some(BinaryOp::Sub),
            AssignOp::Mul => Some(BinaryOp::Mul),
            AssignOp::Div => Some(BinaryOp::Div),
            AssignOp::Rem => Some(BinaryOp::Rem),
        }
    }
}

// ======================================================================
// Expressions
// ======================================================================

#[derive(Debug, Clone, PartialEq)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExprKind {
    Int(u64),
    Float(f64),
    Bool(bool),
    Char(char),
    Str(Vec<StrSegment>),
    /// `none`
    None,
    /// `()`
    Unit,
    /// A bare name: a variable, function, type or enum variant.
    Name(String),
    /// `[a, b, c]`
    List(Vec<Expr>),
    Unary {
        op: UnaryOp,
        expr: Box<Expr>,
    },
    Binary {
        op: BinaryOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    /// `start..<end` or `start...end`
    Range {
        start: Box<Expr>,
        end: Box<Expr>,
        inclusive: bool,
    },
    /// `f(args)`. Also struct construction `Point(x: 1, y: 2)`, enum
    /// variants `Circle(1.0)` and conversions `Float(x)`; the checker
    /// tells them apart.
    Call {
        callee: Box<Expr>,
        args: Vec<Arg>,
    },
    /// `receiver.method(args)`
    MethodCall {
        receiver: Box<Expr>,
        method: Ident,
        args: Vec<Arg>,
    },
    /// `expr.field`
    Field {
        expr: Box<Expr>,
        field: Ident,
    },
    /// `expr[index]`
    Index {
        expr: Box<Expr>,
        index: Box<Expr>,
    },
    /// `expr!`: unwrap an optional, panicking on `none`.
    Unwrap(Box<Expr>),
    /// `take expr` outside an argument list, e.g. `let b = take a`.
    Take(Box<Expr>),
    /// `if cond: then else: else_`. `else_` is a `Block` or another `If`.
    If {
        cond: Box<Expr>,
        then: Block,
        else_: Option<Box<Expr>>,
    },
    Match {
        scrutinee: Box<Expr>,
        arms: Vec<MatchArm>,
    },
    /// A block used as an expression (the body of an `else`).
    Block(Block),
}

#[derive(Debug, Clone, PartialEq)]
pub enum StrSegment {
    Text(String),
    Expr(Expr),
}

/// One argument in a call: `[label:] [mut|take] value`.
#[derive(Debug, Clone, PartialEq)]
pub struct Arg {
    pub label: Option<Ident>,
    pub mode: Mode,
    /// Span of the `mut` / `take` keyword, if written.
    pub mode_span: Option<Span>,
    pub value: Expr,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
}

impl UnaryOp {
    pub fn symbol(self) -> &'static str {
        match self {
            UnaryOp::Neg => "-",
            UnaryOp::Not => "not",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
    /// `a ?? b`
    Coalesce,
}

impl BinaryOp {
    pub fn symbol(self) -> &'static str {
        match self {
            BinaryOp::Add => "+",
            BinaryOp::Sub => "-",
            BinaryOp::Mul => "*",
            BinaryOp::Div => "/",
            BinaryOp::Rem => "%",
            BinaryOp::Eq => "==",
            BinaryOp::Ne => "!=",
            BinaryOp::Lt => "<",
            BinaryOp::Le => "<=",
            BinaryOp::Gt => ">",
            BinaryOp::Ge => ">=",
            BinaryOp::And => "and",
            BinaryOp::Or => "or",
            BinaryOp::Coalesce => "??",
        }
    }

    pub fn is_comparison(self) -> bool {
        matches!(
            self,
            BinaryOp::Eq | BinaryOp::Ne | BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge
        )
    }

    pub fn is_arithmetic(self) -> bool {
        matches!(
            self,
            BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Rem
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatchArm {
    pub pattern: Pattern,
    /// An inline arm `Point: 0.0` becomes a block holding one expression.
    pub body: Block,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Pattern {
    pub kind: PatternKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PatternKind {
    /// `_`
    Wildcard,
    /// A bare name. It's an enum variant if one by that name exists for the
    /// matched type; otherwise it binds the value to a new variable.
    Name(String),
    /// `Circle(r)`, `Rect(w, h)`
    Variant {
        name: Ident,
        args: Vec<Pattern>,
    },
    Int(i64),
    Str(String),
    Bool(bool),
    Char(char),
}
