//! The TDS static checker: types, parameter modes and moves.
//!
//! It runs after parsing and before the interpreter, and reports every
//! error it can find in one pass. It does four jobs:
//!
//! 1. **Declarations.** Collects functions, structs and enums, resolves the
//!    types they mention, and rejects duplicates and infinitely sized
//!    structs.
//! 2. **Types.** Checks every expression *bidirectionally*: where the
//!    context knows what type it wants (a parameter, an annotation, a
//!    return type), that type is passed down as an [`Expect`], which is how
//!    `[]`, `none` and branch values get their types. Otherwise the type is
//!    worked out from the expression itself.
//! 3. **Parameter modes and mutability.** `mut` / `take` markers at call
//!    sites must match the parameters; only `var` variables and `mut`
//!    parameters can change.
//! 4. **Moves.** A flow-sensitive analysis tracks which variables have been
//!    moved away by `take` on each path through the code ([`Flow`]), so
//!    using one afterwards is an error. Branches are merged at the end of
//!    `if` and `match`; a move inside a loop of a variable from outside it
//!    is reported because the next iteration would use it.
//!
//! This is the Phase 1 stand-in for the real ownership checker, which will
//! run on TIR in Phase 3 (borrows, lifetimes, Send/Share).
//!
//! # Error recovery
//!
//! An expression whose type can't be determined gets [`Type::Error`], which
//! fits everywhere, so one mistake produces one diagnostic.

mod builtins;
mod types;

use std::collections::{HashMap, HashSet};

use tds_ast::*;
use tds_diagnostics::{codes, Diagnostic, Severity, Span};

pub use types::Type;

/// Checks a whole file. Returns errors and warnings.
pub fn check_module(module: &Module) -> Vec<Diagnostic> {
    let mut checker = Checker::new();
    let fns = checker.declare_items(module.items.iter(), false);
    for (decl, sig) in fns {
        checker.check_fn(decl, &sig);
    }
    checker.check_main();
    checker.diagnostics
}

/// A checker that keeps its state between REPL entries: declarations and
/// top-level variables persist from one entry to the next.
#[derive(Clone)]
pub struct ReplChecker {
    checker: Checker,
}

impl Default for ReplChecker {
    fn default() -> Self {
        ReplChecker {
            checker: Checker::new(),
        }
    }
}

impl ReplChecker {
    pub fn new() -> ReplChecker {
        ReplChecker::default()
    }

    /// Checks one REPL entry. If it has errors, the session is left as it
    /// was before the entry, so a typo doesn't leave half-declared names
    /// behind.
    pub fn check(&mut self, entries: &[TopLevel]) -> Vec<Diagnostic> {
        let saved = self.checker.clone();
        let items: Vec<&Item> = entries
            .iter()
            .filter_map(|e| match e {
                TopLevel::Item(item) => Some(item),
                TopLevel::Stmt(_) => None,
            })
            .collect();
        let fns = self.checker.declare_items(items, true);
        for (decl, sig) in fns {
            self.checker.check_fn(decl, &sig);
        }
        for entry in entries {
            if let TopLevel::Stmt(stmt) = entry {
                self.checker.check_repl_stmt(stmt);
            }
        }
        let diagnostics = std::mem::take(&mut self.checker.diagnostics);
        if diagnostics.iter().any(|d| d.severity == Severity::Error) {
            self.checker = saved;
        }
        diagnostics
    }
}

// ======================================================================
// Checker state
// ======================================================================

/// What the context expects of an expression's value.
#[derive(Debug, Clone)]
enum Expect {
    /// The value is thrown away (an expression statement).
    Discard,
    /// The value is used, but nothing is known about its type yet.
    Infer,
    /// The value must fit this type.
    Type(Type),
}

impl Expect {
    /// Whether a block checked with this expectation must end in a value.
    fn needs_value(&self) -> bool {
        match self {
            Expect::Discard => false,
            Expect::Infer => true,
            Expect::Type(t) => !matches!(t, Type::Unit | Type::Error),
        }
    }
}

#[derive(Debug, Clone)]
struct ParamSig {
    name: String,
    mode: Mode,
    ty: Type,
}

#[derive(Debug, Clone)]
struct FnSig {
    name: String,
    params: Vec<ParamSig>,
    ret: Type,
    name_span: Span,
}

#[derive(Debug, Clone)]
struct FieldDef {
    name: String,
    ty: Type,
    has_default: bool,
    span: Span,
}

#[derive(Debug, Clone)]
struct StructDef {
    fields: Vec<FieldDef>,
    name_span: Span,
}

#[derive(Debug, Clone)]
struct VariantDef {
    name: String,
    /// Field names are `None` for positional fields.
    fields: Vec<(Option<String>, Type)>,
    span: Span,
}

#[derive(Debug, Clone)]
struct EnumDef {
    variants: Vec<VariantDef>,
    name_span: Span,
}

type VarId = usize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VarKind {
    Let,
    Var,
    Param(Mode),
    LoopVar,
    /// A name bound by a `match` pattern.
    Binding,
}

#[derive(Debug, Clone)]
struct Var {
    name: String,
    ty: Type,
    kind: VarKind,
    span: Span,
    /// How many loops enclose the declaration.
    loop_depth: usize,
}

/// Where a variable was moved.
#[derive(Debug, Clone)]
struct MoveSite {
    span: Span,
    /// The function the value was moved into, if it was a call argument.
    into: Option<String>,
}

/// The state of the move analysis at one point in the program.
#[derive(Debug, Clone)]
struct Flow {
    moved: HashMap<VarId, MoveSite>,
    /// False after `return`, `break`, `continue` or a call to `panic`.
    reachable: bool,
}

impl Flow {
    fn new() -> Flow {
        Flow {
            moved: HashMap::new(),
            reachable: true,
        }
    }

    /// Merges the states at the end of two paths that meet. A variable
    /// moved on either path counts as moved. Unreachable paths don't count.
    fn join(self, other: Flow) -> Flow {
        if !self.reachable {
            return other;
        }
        if !other.reachable {
            return self;
        }
        let mut moved = self.moved;
        for (id, site) in other.moved {
            moved.entry(id).or_insert(site);
        }
        Flow {
            moved,
            reachable: true,
        }
    }
}

#[derive(Debug, Clone, Default)]
struct LoopCtx {
    /// Flow states at each `break`.
    breaks: Vec<Flow>,
    /// Flow states at each `continue`.
    continues: Vec<Flow>,
}

/// What a match pattern covers, for the exhaustiveness check.
#[derive(Debug, Clone, Default)]
struct Coverage {
    /// Matches every value.
    all: bool,
    /// Matches every value of this enum variant.
    variant: Option<String>,
    /// Matches this `Bool` value.
    boolean: Option<bool>,
    /// The pattern had an error; skip the exhaustiveness check.
    error: bool,
}

/// What a mutability check is for, to pick the right error message.
#[derive(Debug, Clone, Copy)]
enum Mutation<'a> {
    Assign,
    MutArg,
    Method(&'a str),
}

#[derive(Clone)]
struct Checker {
    diagnostics: Vec<Diagnostic>,
    fns: HashMap<String, FnSig>,
    structs: HashMap<String, StructDef>,
    enums: HashMap<String, EnumDef>,
    /// Variant name → (enum name, index). Variant names are global in
    /// Phase 1, so `Circle(1.0)` works without writing `Shape.`.
    variants: HashMap<String, (String, usize)>,
    vars: Vec<Var>,
    /// Innermost scope last. Each maps names to variables.
    scopes: Vec<Vec<(String, VarId)>>,
    flow: Flow,
    loops: Vec<LoopCtx>,
    /// The current function's return type; `None` at the REPL top level.
    ret: Option<Type>,
}

/// Built-in type names that user declarations may not reuse.
const BUILTIN_TYPES: &[&str] = &[
    "Int", "Int64", "Float", "Float64", "Bool", "Char", "String", "List", "Option",
];

/// Types from the full language that Phase 1 doesn't have yet.
const LATER_TYPES: &[&str] = &[
    "Int8", "Int16", "Int32", "UInt", "UInt8", "UInt16", "UInt32", "UInt64", "Float32", "Byte",
    "Str", "Array", "Slice", "Map", "Set", "Result", "Box", "Rc", "Arc", "Ptr", "MutPtr", "Error",
];

impl Checker {
    fn new() -> Checker {
        Checker {
            diagnostics: Vec::new(),
            fns: HashMap::new(),
            structs: HashMap::new(),
            enums: HashMap::new(),
            variants: HashMap::new(),
            vars: Vec::new(),
            scopes: vec![Vec::new()],
            flow: Flow::new(),
            loops: Vec::new(),
            ret: None,
        }
    }

    fn error(&mut self, diag: Diagnostic) {
        self.diagnostics.push(diag);
    }

    // ==================================================================
    // Declarations
    // ==================================================================

    /// Registers items and checks their declarations. Returns each
    /// function with its signature, for checking bodies afterwards.
    /// With `replace`, a declaration replaces an earlier one with the same
    /// name instead of being an error (the REPL).
    fn declare_items<'a>(
        &mut self,
        items: impl IntoIterator<Item = &'a Item>,
        replace: bool,
    ) -> Vec<(&'a FnDecl, FnSig)> {
        let items: Vec<&Item> = items.into_iter().collect();

        // Pass 1: names, so types can refer to each other in any order.
        let mut accepted: Vec<&Item> = Vec::new();
        let mut seen: HashMap<String, Span> = HashMap::new();
        for item in &items {
            let name = item.name();
            if BUILTIN_TYPES.contains(&name.name.as_str())
                || LATER_TYPES.contains(&name.name.as_str())
            {
                self.error(
                    Diagnostic::error(
                        codes::DUPLICATE_DEFINITION,
                        format!("`{}` is a built-in type name", name.name),
                    )
                    .with_primary(name.span, "can't be redefined")
                    .with_help("pick a different name"),
                );
                continue;
            }
            if let Some(first) = seen.get(&name.name) {
                let first = *first;
                self.error(
                    Diagnostic::error(
                        codes::DUPLICATE_DEFINITION,
                        format!("`{}` is defined more than once", name.name),
                    )
                    .with_primary(name.span, "second definition")
                    .with_secondary(first, "first defined here")
                    .with_help("rename one of them"),
                );
                continue;
            }
            if !replace {
                if let Some(first) = self.item_span(&name.name) {
                    self.error(
                        Diagnostic::error(
                            codes::DUPLICATE_DEFINITION,
                            format!("`{}` is defined more than once", name.name),
                        )
                        .with_primary(name.span, "second definition")
                        .with_secondary(first, "first defined here"),
                    );
                    continue;
                }
            }
            seen.insert(name.name.clone(), name.span);
            if replace {
                self.forget_item(&name.name);
            }
            match item {
                Item::Struct(s) => {
                    self.structs.insert(
                        s.name.name.clone(),
                        StructDef {
                            fields: Vec::new(),
                            name_span: s.name.span,
                        },
                    );
                }
                Item::Enum(e) => {
                    self.enums.insert(
                        e.name.name.clone(),
                        EnumDef {
                            variants: Vec::new(),
                            name_span: e.name.span,
                        },
                    );
                }
                Item::Fn(_) => {}
            }
            accepted.push(item);
        }

        // Pass 2: resolve the types inside each declaration.
        let mut fns = Vec::new();
        for item in &accepted {
            match item {
                Item::Struct(s) => self.declare_struct(s),
                Item::Enum(e) => self.declare_enum(e),
                Item::Fn(f) => {
                    let sig = self.fn_signature(f);
                    self.fns.insert(f.name.name.clone(), sig.clone());
                    fns.push((f, sig));
                }
            }
        }

        // Pass 3: whole-declaration checks.
        let names: Vec<String> = accepted
            .iter()
            .filter(|i| !matches!(i, Item::Fn(_)))
            .map(|i| i.name().name.clone())
            .collect();
        self.check_recursive_types(&names);
        for item in &accepted {
            if let Item::Struct(s) = item {
                self.check_field_defaults(s);
            }
        }
        fns
    }

    /// The span of an existing declaration with this name, if any.
    fn item_span(&self, name: &str) -> Option<Span> {
        if let Some(f) = self.fns.get(name) {
            return Some(f.name_span);
        }
        if let Some(s) = self.structs.get(name) {
            return Some(s.name_span);
        }
        self.enums.get(name).map(|e| e.name_span)
    }

    /// Removes a declaration (REPL redefinition).
    fn forget_item(&mut self, name: &str) {
        self.fns.remove(name);
        self.structs.remove(name);
        if self.enums.remove(name).is_some() {
            self.variants.retain(|_, (enum_name, _)| enum_name != name);
        }
    }

    fn declare_struct(&mut self, s: &StructDecl) {
        let mut fields: Vec<FieldDef> = Vec::new();
        for field in &s.fields {
            if let Some(first) = fields.iter().find(|f| f.name == field.name.name) {
                let first = first.span;
                self.error(
                    Diagnostic::error(
                        codes::DUPLICATE_DEFINITION,
                        format!("field `{}` is declared twice", field.name.name),
                    )
                    .with_primary(field.name.span, "second declaration")
                    .with_secondary(first, "first declared here"),
                );
                continue;
            }
            let ty = self.resolve_type(&field.ty);
            fields.push(FieldDef {
                name: field.name.name.clone(),
                ty,
                has_default: field.default.is_some(),
                span: field.name.span,
            });
        }
        if let Some(def) = self.structs.get_mut(&s.name.name) {
            def.fields = fields;
        }
    }

    fn declare_enum(&mut self, e: &EnumDecl) {
        let mut variants: Vec<VariantDef> = Vec::new();
        for v in &e.variants {
            if let Some((other_enum, _)) = self.variants.get(&v.name.name) {
                let other = other_enum.clone();
                let message = if other == e.name.name {
                    format!("variant `{}` is declared twice", v.name.name)
                } else {
                    format!(
                        "variant `{}` is also declared in enum `{other}`",
                        v.name.name
                    )
                };
                self.error(
                    Diagnostic::error(codes::DUPLICATE_DEFINITION, message)
                        .with_primary(v.name.span, "duplicate variant")
                        .with_note("in Phase 1, variant names are used without the enum name, so they must be unique"),
                );
                continue;
            }
            if self.structs.contains_key(&v.name.name) || self.fns.contains_key(&v.name.name) {
                self.error(
                    Diagnostic::error(
                        codes::DUPLICATE_DEFINITION,
                        format!(
                            "variant `{}` has the same name as another declaration",
                            v.name.name
                        ),
                    )
                    .with_primary(v.name.span, "name already used"),
                );
                continue;
            }
            let mut fields = Vec::new();
            let mut names: HashSet<String> = HashSet::new();
            for field in &v.fields {
                if let Some(name) = &field.name {
                    if !names.insert(name.name.clone()) {
                        self.error(
                            Diagnostic::error(
                                codes::DUPLICATE_DEFINITION,
                                format!("field `{}` is declared twice", name.name),
                            )
                            .with_primary(name.span, "second declaration"),
                        );
                    }
                }
                let ty = self.resolve_type(&field.ty);
                fields.push((field.name.as_ref().map(|n| n.name.clone()), ty));
            }
            self.variants
                .insert(v.name.name.clone(), (e.name.name.clone(), variants.len()));
            variants.push(VariantDef {
                name: v.name.name.clone(),
                fields,
                span: v.name.span,
            });
        }
        if let Some(def) = self.enums.get_mut(&e.name.name) {
            def.variants = variants;
        }
    }

    fn fn_signature(&mut self, f: &FnDecl) -> FnSig {
        let mut params: Vec<ParamSig> = Vec::new();
        let mut seen: HashMap<&str, Span> = HashMap::new();
        for p in &f.params {
            if let Some(first) = seen.get(p.name.name.as_str()) {
                let first = *first;
                self.error(
                    Diagnostic::error(
                        codes::DUPLICATE_PARAMETER,
                        format!("parameter `{}` appears twice", p.name.name),
                    )
                    .with_primary(p.name.span, "second parameter with this name")
                    .with_secondary(first, "first one here"),
                );
            }
            seen.insert(&p.name.name, p.name.span);
            let ty = self.resolve_type(&p.ty);
            params.push(ParamSig {
                name: p.name.name.clone(),
                mode: p.mode,
                ty,
            });
        }
        let ret = match &f.ret {
            Some(t) => self.resolve_type(t),
            None => Type::Unit,
        };
        FnSig {
            name: f.name.name.clone(),
            params,
            ret,
            name_span: f.name.span,
        }
    }

    fn resolve_type(&mut self, t: &TypeExpr) -> Type {
        match &t.kind {
            TypeExprKind::Unit => Type::Unit,
            TypeExprKind::Optional(inner) => {
                let inner_ty = self.resolve_type(inner);
                if matches!(inner_ty, Type::Optional(_)) {
                    self.error(
                        Diagnostic::error(codes::NESTED_OPTIONAL, "optionals can't be nested")
                            .with_primary(t.span, "an optional of an optional")
                            .with_help("use a single `?`"),
                    );
                    return Type::Error;
                }
                Type::optional(inner_ty)
            }
            TypeExprKind::Named { name, args } => {
                let expect_args = |checker: &mut Checker, n: usize| -> bool {
                    if args.len() == n {
                        return true;
                    }
                    let message = match n {
                        0 => format!("`{}` doesn't take type arguments", name.name),
                        1 => format!(
                            "`{}` needs one type argument, like `{}[Int]`",
                            name.name, name.name
                        ),
                        _ => format!("`{}` needs {n} type arguments", name.name),
                    };
                    checker.error(
                        Diagnostic::error(codes::WRONG_TYPE_ARGUMENTS, message)
                            .with_primary(t.span, "wrong number of type arguments"),
                    );
                    false
                };
                match name.name.as_str() {
                    "Int" | "Int64" | "Float" | "Float64" | "Bool" | "Char" | "String" => {
                        if !expect_args(self, 0) {
                            return Type::Error;
                        }
                        match name.name.as_str() {
                            "Int" | "Int64" => Type::Int,
                            "Float" | "Float64" => Type::Float,
                            "Bool" => Type::Bool,
                            "Char" => Type::Char,
                            _ => Type::String,
                        }
                    }
                    "List" => {
                        if !expect_args(self, 1) {
                            return Type::Error;
                        }
                        Type::list(self.resolve_type(&args[0]))
                    }
                    "Option" => {
                        if !expect_args(self, 1) {
                            return Type::Error;
                        }
                        let inner = self.resolve_type(&args[0]);
                        if matches!(inner, Type::Optional(_)) {
                            self.error(
                                Diagnostic::error(
                                    codes::NESTED_OPTIONAL,
                                    "optionals can't be nested",
                                )
                                .with_primary(t.span, "an optional of an optional"),
                            );
                            return Type::Error;
                        }
                        Type::optional(inner)
                    }
                    other if LATER_TYPES.contains(&other) => {
                        let help = if other.starts_with("Int")
                            || other.starts_with("UInt")
                            || other == "Byte"
                        {
                            "use `Int` (64-bit, signed) for now"
                        } else if other == "Float32" {
                            "use `Float` (64-bit) for now"
                        } else if other == "Str" {
                            "use `String` for now"
                        } else {
                            "this type arrives in a later phase"
                        };
                        self.error(
                            Diagnostic::error(
                                codes::UNSUPPORTED_TYPE,
                                format!("`{other}` isn't available in the Phase 1 prototype"),
                            )
                            .with_primary(name.span, "not supported yet")
                            .with_note("Phase 1 has Int, Float, Bool, Char, String, List[T], T? and your own structs and enums")
                            .with_help(help),
                        );
                        Type::Error
                    }
                    other => {
                        if self.structs.contains_key(other) {
                            expect_args(self, 0);
                            Type::Struct(other.to_string())
                        } else if self.enums.contains_key(other) {
                            expect_args(self, 0);
                            Type::Enum(other.to_string())
                        } else {
                            let mut candidates: Vec<String> =
                                BUILTIN_TYPES.iter().map(|s| s.to_string()).collect();
                            candidates.extend(self.structs.keys().cloned());
                            candidates.extend(self.enums.keys().cloned());
                            let mut diag = Diagnostic::error(
                                codes::UNKNOWN_TYPE,
                                format!("unknown type `{other}`"),
                            )
                            .with_primary(name.span, "not a known type");
                            if let Some(s) =
                                did_you_mean(other, candidates.iter().map(String::as_str))
                            {
                                diag = diag.with_help(format!("did you mean `{s}`?"));
                            }
                            self.error(diag);
                            Type::Error
                        }
                    }
                }
            }
        }
    }

    /// Rejects structs and enums that contain themselves directly (not
    /// through a `List`), because they would be infinitely large.
    fn check_recursive_types(&mut self, names: &[String]) {
        let mut reported: HashSet<String> = HashSet::new();
        for name in names {
            if reported.contains(name) {
                continue;
            }
            for (field_name, field_ty, field_span) in self.direct_fields(name) {
                let mut path = vec![name.clone()];
                let Some(child) = direct_type_name(&field_ty) else {
                    continue;
                };
                if self.reaches(&child, name, &mut path, &mut HashSet::new()) {
                    let cycle = if path.len() == 1 {
                        format!("`{name}` contains a `{name}`")
                    } else {
                        let mut chain = path.join("` → `");
                        chain.push_str(&format!("` → `{name}"));
                        format!("`{chain}`")
                    };
                    self.error(
                        Diagnostic::error(codes::RECURSIVE_STRUCT, format!("`{name}` contains itself, so it would be infinitely large"))
                            .with_primary(field_span, format!("`{field_name}` holds a `{child}` directly"))
                            .with_note(format!("the cycle is {cycle}"))
                            .with_help(format!("store it in a list instead, like `List[{child}]`; `Box` arrives in Phase 4")),
                    );
                    reported.extend(path);
                    break;
                }
            }
        }
    }

    /// Fields of a struct or enum (all variants), with their spans.
    fn direct_fields(&self, name: &str) -> Vec<(String, Type, Span)> {
        if let Some(s) = self.structs.get(name) {
            return s
                .fields
                .iter()
                .map(|f| (f.name.clone(), f.ty.clone(), f.span))
                .collect();
        }
        if let Some(e) = self.enums.get(name) {
            return e
                .variants
                .iter()
                .flat_map(|v| {
                    v.fields.iter().enumerate().map(|(i, (n, t))| {
                        (
                            n.clone().unwrap_or_else(|| format!("{}.{i}", v.name)),
                            t.clone(),
                            v.span,
                        )
                    })
                })
                .collect();
        }
        Vec::new()
    }

    /// Whether `from` contains `target` directly, recording the path.
    fn reaches(
        &self,
        from: &str,
        target: &str,
        path: &mut Vec<String>,
        seen: &mut HashSet<String>,
    ) -> bool {
        if from == target {
            return true;
        }
        if !seen.insert(from.to_string()) {
            return false;
        }
        path.push(from.to_string());
        for (_, ty, _) in self.direct_fields(from) {
            if let Some(child) = direct_type_name(&ty) {
                if self.reaches(&child, target, path, seen) {
                    return true;
                }
            }
        }
        path.pop();
        false
    }

    fn check_field_defaults(&mut self, s: &StructDecl) {
        for field in &s.fields {
            let Some(default) = &field.default else {
                continue;
            };
            let Some(ty) = self
                .structs
                .get(&s.name.name)
                .and_then(|d| d.fields.iter().find(|f| f.name == field.name.name))
                .map(|f| f.ty.clone())
            else {
                continue;
            };
            // Defaults are checked with no variables in scope.
            let saved_scopes = std::mem::replace(&mut self.scopes, vec![Vec::new()]);
            let saved_flow = std::mem::replace(&mut self.flow, Flow::new());
            self.expr(default, &Expect::Type(ty));
            self.scopes = saved_scopes;
            self.flow = saved_flow;
        }
    }

    fn check_main(&mut self) {
        let Some(main) = self.fns.get("main").cloned() else {
            self.error(
                Diagnostic::error(codes::NO_MAIN, "this program has no `main` function")
                    .with_note("a program starts running at `fn main()`")
                    .with_help("add one:\n\n    fn main():\n        print(\"Hello!\")"),
            );
            return;
        };
        if !main.params.is_empty() || main.ret != Type::Unit {
            self.error(
                Diagnostic::error(
                    codes::BAD_MAIN_SIGNATURE,
                    "`main` must take no parameters and return nothing",
                )
                .with_primary(main.name_span, "wrong signature")
                .with_help("write it as `fn main():`; read command-line arguments with `args()`"),
            );
        }
    }

    // ==================================================================
    // Functions and scopes
    // ==================================================================

    fn check_fn(&mut self, f: &FnDecl, sig: &FnSig) {
        // A function body sees only its parameters: not the REPL's
        // variables, and no state from other functions.
        let saved_scopes = std::mem::replace(&mut self.scopes, vec![Vec::new()]);
        let saved_flow = std::mem::replace(&mut self.flow, Flow::new());
        let saved_loops = std::mem::take(&mut self.loops);
        let saved_ret = self.ret.replace(sig.ret.clone());

        for (param, decl) in sig.params.iter().zip(&f.params) {
            self.declare(
                &param.name,
                param.ty.clone(),
                VarKind::Param(param.mode),
                decl.name.span,
            );
        }
        let expect = if sig.ret == Type::Unit {
            Expect::Discard
        } else {
            Expect::Type(sig.ret.clone())
        };
        self.block(&f.body, &expect);

        self.scopes = saved_scopes;
        self.flow = saved_flow;
        self.loops = saved_loops;
        self.ret = saved_ret;
    }

    fn check_repl_stmt(&mut self, stmt: &Stmt) {
        match &stmt.kind {
            // A bare expression at the REPL prints its value, unless it's
            // an `if` without `else`, which has no value.
            StmtKind::Expr(e) => {
                let expect = match &e.kind {
                    ExprKind::If { else_: None, .. } => Expect::Discard,
                    _ => Expect::Infer,
                };
                self.expr(e, &expect);
            }
            _ => self.stmt(stmt),
        }
        // A `return` at the top level is reported; don't let it make the
        // rest of the session unreachable.
        self.flow.reachable = true;
    }

    fn declare(&mut self, name: &str, ty: Type, kind: VarKind, span: Span) -> VarId {
        let id = self.vars.len();
        self.vars.push(Var {
            name: name.to_string(),
            ty,
            kind,
            span,
            loop_depth: self.loops.len(),
        });
        self.scopes.last_mut().unwrap().push((name.to_string(), id));
        id
    }

    fn lookup_var(&self, name: &str) -> Option<VarId> {
        self.scopes
            .iter()
            .rev()
            .flat_map(|scope| scope.iter().rev())
            .find(|(n, _)| n == name)
            .map(|(_, id)| *id)
    }

    fn in_scope<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        self.scopes.push(Vec::new());
        let result = f(self);
        self.scopes.pop();
        result
    }

    // ==================================================================
    // Statements
    // ==================================================================

    /// Checks a block. Returns its type: the type of its final expression,
    /// `()` if it has none, or `Never` if its end can't be reached.
    fn block(&mut self, block: &Block, expect: &Expect) -> Type {
        self.in_scope(|c| {
            let mut ty = Type::Unit;
            let mut has_tail = false;
            for (i, stmt) in block.stmts.iter().enumerate() {
                let is_last = i + 1 == block.stmts.len();
                match &stmt.kind {
                    StmtKind::Expr(e) if is_last && !matches!(expect, Expect::Discard) => {
                        let tail_expect = match expect {
                            Expect::Type(Type::Unit) => Expect::Discard,
                            other => other.clone(),
                        };
                        ty = c.expr(e, &tail_expect);
                        has_tail = !matches!(tail_expect, Expect::Discard);
                    }
                    _ => c.stmt(stmt),
                }
            }
            if !c.flow.reachable {
                return Type::Never;
            }
            if has_tail {
                return match expect {
                    Expect::Type(t) => t.clone(),
                    _ => ty,
                };
            }
            if let (true, Expect::Type(want)) = (expect.needs_value(), expect) {
                let end = block.stmts.last().map_or(block.span, |s| s.span);
                c.error(
                    Diagnostic::error(
                        codes::MISSING_RETURN,
                        format!("expected a value of type `{want}`, but this can end without one"),
                    )
                    .with_primary(
                        end,
                        format!("a value of type `{want}` is needed after this"),
                    )
                    .with_note("a block's value is its last expression")
                    .with_help(format!(
                        "end with an expression of type `{want}`, or `return` one"
                    )),
                );
                return Type::Error;
            }
            Type::Unit
        })
    }

    fn stmt(&mut self, stmt: &Stmt) {
        match &stmt.kind {
            StmtKind::Let {
                mutable,
                name,
                ty,
                init,
            } => {
                let var_ty = match ty {
                    Some(annotation) => {
                        let t = self.resolve_type(annotation);
                        self.expr(init, &Expect::Type(t.clone()));
                        t
                    }
                    None => match self.expr(init, &Expect::Infer) {
                        Type::Never => Type::Error,
                        t => t,
                    },
                };
                let kind = if *mutable { VarKind::Var } else { VarKind::Let };
                self.declare(&name.name, var_ty, kind, name.span);
            }
            StmtKind::Assign { target, op, value } => self.assign(target, *op, value),
            StmtKind::Expr(e) => {
                self.expr(e, &Expect::Discard);
            }
            StmtKind::While { cond, body } => {
                self.loop_body(|c| {
                    c.condition(cond, "while");
                    c.block(body, &Expect::Discard);
                });
            }
            StmtKind::For { var, iter, body } => {
                let iter_ty = self.expr(iter, &Expect::Infer);
                let elem = match &iter_ty {
                    Type::List(elem) => (**elem).clone(),
                    Type::Range => Type::Int,
                    Type::String => Type::Char,
                    Type::Error => Type::Error,
                    other => {
                        let mut diag = Diagnostic::error(
                            codes::NOT_ITERABLE,
                            format!("can't loop over a value of type `{other}`"),
                        )
                        .with_primary(iter.span, format!("this has type `{other}`"))
                        .with_note("`for` works on lists, ranges and strings");
                        if *other == Type::Int {
                            diag = diag.with_help("to count, use a range: `for i in 0..<n:`");
                        } else if let Type::Optional(_) = other {
                            diag = diag.with_help("unwrap the optional first with `!` or `??`");
                        }
                        self.error(diag);
                        Type::Error
                    }
                };
                self.loop_body(|c| {
                    c.in_scope(|c| {
                        c.declare(&var.name, elem, VarKind::LoopVar, var.span);
                        c.block(body, &Expect::Discard);
                    });
                });
            }
            StmtKind::Return(value) => self.return_stmt(value.as_ref(), stmt.span),
            StmtKind::Break | StmtKind::Continue => {
                let is_break = matches!(stmt.kind, StmtKind::Break);
                let flow = self.flow.clone();
                match self.loops.last_mut() {
                    Some(ctx) if is_break => ctx.breaks.push(flow),
                    Some(ctx) => ctx.continues.push(flow),
                    None => {
                        let word = if is_break { "break" } else { "continue" };
                        self.error(
                            Diagnostic::error(
                                codes::BREAK_OUTSIDE_LOOP,
                                format!("`{word}` outside a loop"),
                            )
                            .with_primary(
                                stmt.span,
                                format!("`{word}` must be inside `while` or `for`"),
                            ),
                        );
                    }
                }
                self.flow.reachable = false;
            }
        }
    }

    /// Checks a loop body (and condition), then the moves it makes.
    fn loop_body(&mut self, check: impl FnOnce(&mut Self)) {
        let before = self.flow.clone();
        self.loops.push(LoopCtx::default());
        check(self);
        let ctx = self.loops.pop().unwrap();

        let mut end = self.flow.clone();
        for state in ctx.continues {
            end = end.join(state);
        }

        // A variable from outside the loop that is still moved at the end
        // of an iteration would be used after the move in the next one.
        if end.reachable {
            let mut moved_here: Vec<(VarId, MoveSite)> = end
                .moved
                .iter()
                .filter(|(id, _)| {
                    !before.moved.contains_key(id) && self.vars[**id].loop_depth <= self.loops.len()
                })
                .map(|(id, site)| (*id, site.clone()))
                .collect();
            moved_here.sort_by_key(|(_, site)| site.span.start);
            for (id, site) in moved_here {
                let name = self.vars[id].name.clone();
                self.error(
                    Diagnostic::error(
                        codes::MOVED_IN_LOOP,
                        format!("`{name}` is moved inside a loop"),
                    )
                    .with_primary(site.span, "moved here, in one iteration")
                    .with_secondary(
                        self.vars[id].span,
                        format!("`{name}` is declared outside the loop"),
                    )
                    .with_note("the next iteration would use the value after it was moved")
                    .with_help(format!("move a copy instead: `take {name}.clone()`"))
                    .with_help("or give it a new value before the iteration ends"),
                );
                end.moved.remove(&id);
            }
        }

        let mut after = before.join(end);
        for state in ctx.breaks {
            after = after.join(state);
        }
        self.flow = after;
    }

    fn condition(&mut self, cond: &Expr, keyword: &str) {
        let t = self.expr(cond, &Expect::Infer);
        if !matches!(t, Type::Bool | Type::Error | Type::Never) {
            let mut diag = Diagnostic::error(
                codes::CONDITION_NOT_BOOL,
                format!("`{keyword}` condition must be a `Bool`"),
            )
            .with_primary(cond.span, format!("this has type `{t}`"))
            .with_note("TDS doesn't treat numbers, strings or lists as true or false");
            diag = match t {
                Type::Int | Type::Float => diag.with_help("compare it explicitly, like `x != 0`"),
                Type::Optional(_) => diag.with_help("check for a value with `x != none`"),
                Type::List(_) | Type::String => {
                    diag.with_help("check its length, like `not x.is_empty()`")
                }
                _ => diag,
            };
            self.error(diag);
        }
    }

    fn return_stmt(&mut self, value: Option<&Expr>, span: Span) {
        match (self.ret.clone(), value) {
            (None, value) => {
                if let Some(e) = value {
                    self.expr(e, &Expect::Infer);
                }
                self.error(
                    Diagnostic::error(codes::BAD_RETURN, "`return` outside a function")
                        .with_primary(span, "nothing to return from"),
                );
            }
            (Some(Type::Unit), Some(e)) => {
                let t = self.expr(e, &Expect::Infer);
                if !matches!(t, Type::Unit | Type::Never | Type::Error) {
                    self.error(
                        Diagnostic::error(
                            codes::BAD_RETURN,
                            "this function doesn't return a value",
                        )
                        .with_primary(e.span, format!("returns `{t}`"))
                        .with_help(format!(
                            "add `-> {t}` to the function's signature, or remove the value"
                        )),
                    );
                }
            }
            (Some(Type::Unit), None) => {}
            (Some(ret), None) => {
                self.error(
                    Diagnostic::error(
                        codes::BAD_RETURN,
                        format!("this function must return a value of type `{ret}`"),
                    )
                    .with_primary(span, "`return` without a value")
                    .with_help("return a value: `return ...`"),
                );
            }
            (Some(ret), Some(e)) => {
                self.expr(e, &Expect::Type(ret));
            }
        }
        self.flow.reachable = false;
    }

    fn assign(&mut self, target: &Expr, op: AssignOp, value: &Expr) {
        let Some(root) = self.place_root(target) else {
            // `f().x = 1` and similar.
            self.expr(value, &Expect::Infer);
            if !matches!(target.kind, ExprKind::Name(_)) {
                self.expr(target, &Expect::Infer);
                self.error(
                    Diagnostic::error(
                        codes::INVALID_ASSIGNMENT_TARGET,
                        "can't assign to a temporary value",
                    )
                    .with_primary(target.span, "this value isn't stored in a variable"),
                );
            } else {
                // An unknown name: let the normal lookup report it.
                self.expr(target, &Expect::Infer);
            }
            return;
        };

        let whole_variable = matches!(target.kind, ExprKind::Name(_));
        if whole_variable && op == AssignOp::Assign {
            // Plain `x = value` doesn't read `x`, and gives a moved
            // variable a new value.
            let ty = self.vars[root].ty.clone();
            self.expr(value, &Expect::Type(ty));
            self.check_mutable(root, target.span, Mutation::Assign);
            self.flow.moved.remove(&root);
            return;
        }

        let value_ty = self.expr(value, &Expect::Infer);
        let target_ty = self.expr(target, &Expect::Infer);
        self.check_mutable(root, target.span, Mutation::Assign);
        match op.binary_op() {
            None => {
                if !value_ty.fits(&target_ty) {
                    self.mismatch(&value_ty, &target_ty, value);
                }
            }
            Some(bin) => {
                self.arithmetic(
                    bin,
                    &target_ty,
                    &value_ty,
                    target.span.to(value.span),
                    value,
                );
            }
        }
    }

    // ==================================================================
    // Expressions
    // ==================================================================

    fn expr(&mut self, e: &Expr, expect: &Expect) -> Type {
        let t = self.expr_inner(e, expect);
        if t == Type::Never {
            self.flow.reachable = false;
        }
        // `if`, `match` and blocks check each branch against the
        // expectation themselves.
        let checks_itself = matches!(
            e.kind,
            ExprKind::If { .. } | ExprKind::Match { .. } | ExprKind::Block(_)
        );
        if let (Expect::Type(want), false) = (expect, checks_itself) {
            if !t.fits(want) {
                self.mismatch(&t, want, e);
            }
        }
        t
    }

    /// Reports that `found` doesn't fit `want`.
    fn mismatch(&mut self, found: &Type, want: &Type, e: &Expr) {
        if *found == Type::Unit {
            self.error(
                Diagnostic::error(
                    codes::MISSING_RETURN,
                    format!("expected a value of type `{want}`, but this has no value"),
                )
                .with_primary(e.span, "this produces `()`, not a value")
                .with_note("a function body's value is its last expression")
                .with_help(format!(
                    "end with an expression of type `{want}`, or `return` one"
                )),
            );
            return;
        }
        let numeric_mix = matches!(
            (found, want.unwrap_optional().unwrap_or(want)),
            (Type::Int, Type::Float) | (Type::Float, Type::Int)
        );
        if numeric_mix {
            let target = want.unwrap_optional().unwrap_or(want);
            let help = match (&e.kind, target) {
                (ExprKind::Int(n), Type::Float) => format!("write it as a float: `{n}.0`"),
                (_, Type::Float) => "convert it: `Float(...)`".to_string(),
                _ => "convert it: `Int(...)` (this truncates toward zero)".to_string(),
            };
            self.error(
                Diagnostic::error(
                    codes::NO_IMPLICIT_CONVERSION,
                    format!("expected `{want}`, found `{found}`"),
                )
                .with_primary(e.span, format!("this has type `{found}`"))
                .with_note("TDS never converts between number types automatically")
                .with_help(help),
            );
            return;
        }
        let mut diag = Diagnostic::error(
            codes::MISMATCHED_TYPES,
            format!("expected `{want}`, found `{found}`"),
        )
        .with_primary(e.span, format!("expected `{want}`"));
        if let (Type::Optional(inner), _) = (found, want) {
            if inner.fits(want) {
                diag = diag
                    .with_note("this value might be `none`")
                    .with_help("unwrap it with `!`, or give a default with `?? value`");
            }
        }
        if *want == Type::String && found != &Type::Error {
            diag = diag.with_help(
                "to turn a value into text, use `String(...)` or interpolation: `\"{x}\"`",
            );
        }
        self.error(diag);
    }

    fn expr_inner(&mut self, e: &Expr, expect: &Expect) -> Type {
        match &e.kind {
            ExprKind::Int(n) => {
                if i64::try_from(*n).is_err() {
                    self.error(
                        Diagnostic::error(
                            codes::LITERAL_OUT_OF_RANGE,
                            "integer literal is too large for `Int`",
                        )
                        .with_primary(e.span, "doesn't fit")
                        .with_note(format!("the largest `Int` is {}", i64::MAX)),
                    );
                    return Type::Error;
                }
                Type::Int
            }
            ExprKind::Float(_) => Type::Float,
            ExprKind::Bool(_) => Type::Bool,
            ExprKind::Char(_) => Type::Char,
            ExprKind::Unit => Type::Unit,
            ExprKind::Str(segments) => {
                for segment in segments {
                    if let StrSegment::Expr(inner) = segment {
                        self.expr(inner, &Expect::Infer);
                    }
                }
                Type::String
            }
            ExprKind::None => self.none_literal(e, expect),
            ExprKind::Name(name) => self.name_expr(name, e.span),
            ExprKind::List(items) => self.list_literal(items, e.span, expect),
            ExprKind::Unary { op, expr } => self.unary(*op, expr, e.span),
            ExprKind::Binary { op, lhs, rhs } => self.binary(*op, lhs, rhs, e),
            ExprKind::Range { start, end, .. } => {
                self.expr(start, &Expect::Type(Type::Int));
                self.expr(end, &Expect::Type(Type::Int));
                Type::Range
            }
            ExprKind::Call { callee, args } => self.call(callee, args, e.span),
            ExprKind::MethodCall {
                receiver,
                method,
                args,
            } => self.method_call(receiver, method, args, e.span),
            ExprKind::Field { expr, field } => self.field(expr, field),
            ExprKind::Index { expr, index } => self.index(expr, index),
            ExprKind::Unwrap(inner) => {
                let t = self.expr(inner, &Expect::Infer);
                match t {
                    Type::Optional(inner_ty) => *inner_ty,
                    Type::Error => Type::Error,
                    other => {
                        self.error(
                            Diagnostic::error(
                                codes::NOT_OPTIONAL,
                                format!("`!` unwraps an optional, but this has type `{other}`"),
                            )
                            .with_primary(inner.span, "not optional, so it's never `none`")
                            .with_help("remove the `!`"),
                        );
                        other
                    }
                }
            }
            ExprKind::Take(inner) => self.take(inner, e.span, None),
            ExprKind::If { cond, then, else_ } => {
                self.if_expr(cond, then, else_.as_deref(), e.span, expect)
            }
            ExprKind::Match { scrutinee, arms } => self.match_expr(scrutinee, arms, expect),
            ExprKind::Block(block) => self.block(block, expect),
        }
    }

    fn none_literal(&mut self, e: &Expr, expect: &Expect) -> Type {
        match expect {
            Expect::Type(t @ (Type::Optional(_) | Type::Error)) => t.clone(),
            Expect::Type(other) => {
                let other = other.clone();
                self.error(
                    Diagnostic::error(
                        codes::NOT_OPTIONAL,
                        format!("`none` isn't a valid `{other}`"),
                    )
                    .with_primary(e.span, format!("type `{other}` is expected here"))
                    .with_note("only optional types like `Int?` can be `none`")
                    .with_help(format!("make the type optional: `{other}?`")),
                );
                Type::Error
            }
            _ => {
                self.error(
                    Diagnostic::error(codes::CANNOT_INFER, "can't tell what type this `none` is")
                        .with_primary(e.span, "type unknown")
                        .with_help("add a type annotation, like `var x: Int? = none`"),
                );
                Type::Error
            }
        }
    }

    fn name_expr(&mut self, name: &str, span: Span) -> Type {
        if let Some(id) = self.lookup_var(name) {
            self.use_var(id, span);
            return self.vars[id].ty.clone();
        }
        if let Some((enum_name, index)) = self.variants.get(name).cloned() {
            let variant = &self.enums[&enum_name].variants[index];
            if !variant.fields.is_empty() {
                let count = variant.fields.len();
                self.error(
                    Diagnostic::error(
                        codes::VARIANT_ARGUMENTS,
                        format!("`{name}` needs its field values"),
                    )
                    .with_primary(span, format!("`{name}` has {count} field{}", plural(count)))
                    .with_help(format!("write `{name}(...)`")),
                );
            }
            return Type::Enum(enum_name);
        }
        if self.fns.contains_key(name) || builtins::FUNCTIONS.contains(&name) {
            self.error(
                Diagnostic::error(
                    codes::NOT_A_VALUE,
                    format!("`{name}` is a function, not a value"),
                )
                .with_primary(span, "function used as a value")
                .with_note("functions can't be stored in variables until Phase 4")
                .with_help(format!("to call it, write `{name}(...)`")),
            );
            return Type::Error;
        }
        if self.structs.contains_key(name) || self.enums.contains_key(name) {
            let help = if self.structs.contains_key(name) {
                format!("to create one, write `{name}(field: value, ...)`")
            } else {
                format!("write one of its variants, like `{name}.Variant`")
            };
            self.error(
                Diagnostic::error(
                    codes::NOT_A_VALUE,
                    format!("`{name}` is a type, not a value"),
                )
                .with_primary(span, "type used as a value")
                .with_help(help),
            );
            return Type::Error;
        }
        let candidates = self.value_names();
        let mut diag = Diagnostic::error(codes::UNKNOWN_NAME, format!("unknown name `{name}`"))
            .with_primary(span, "not found in this scope");
        if let Some(s) = did_you_mean(name, candidates.iter().map(String::as_str)) {
            diag = diag.with_help(format!("did you mean `{s}`?"));
        }
        self.error(diag);
        Type::Error
    }

    /// Every name usable as a value or callee here, for suggestions.
    fn value_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .scopes
            .iter()
            .flatten()
            .map(|(n, _)| n.clone())
            .collect();
        names.extend(self.fns.keys().cloned());
        names.extend(self.variants.keys().cloned());
        names.extend(self.structs.keys().cloned());
        names.extend(builtins::FUNCTIONS.iter().map(|s| s.to_string()));
        names
    }

    fn list_literal(&mut self, items: &[Expr], span: Span, expect: &Expect) -> Type {
        let wanted_elem = match expect {
            Expect::Type(Type::List(elem)) => Some((**elem).clone()),
            Expect::Type(Type::Optional(inner)) => match &**inner {
                Type::List(elem) => Some((**elem).clone()),
                _ => None,
            },
            Expect::Type(Type::Error) => Some(Type::Error),
            _ => None,
        };
        if let Some(elem) = wanted_elem {
            for item in items {
                self.expr(item, &Expect::Type(elem.clone()));
            }
            return Type::list(elem);
        }
        let Some((first, rest)) = items.split_first() else {
            self.error(
                Diagnostic::error(codes::CANNOT_INFER, "can't tell what type of list this is")
                    .with_primary(span, "empty list")
                    .with_help("add a type annotation, like `var xs: List[Int] = []`"),
            );
            return Type::Error;
        };
        let elem = match self.expr(first, &Expect::Infer) {
            Type::Never => Type::Error,
            t => t,
        };
        for item in rest {
            self.expr(item, &Expect::Type(elem.clone()));
        }
        Type::list(elem)
    }

    fn unary(&mut self, op: UnaryOp, operand: &Expr, span: Span) -> Type {
        // `-9223372036854775808` is the one literal that only fits negated.
        if let (UnaryOp::Neg, ExprKind::Int(n)) = (op, &operand.kind) {
            if *n == 1u64 << 63 {
                return Type::Int;
            }
        }
        let t = self.expr(operand, &Expect::Infer);
        match (op, &t) {
            (_, Type::Error) => Type::Error,
            (UnaryOp::Neg, Type::Int | Type::Float) => t,
            (UnaryOp::Not, Type::Bool) => Type::Bool,
            (UnaryOp::Neg, _) => {
                self.error(
                    Diagnostic::error(
                        codes::BAD_UNARY_OPERAND,
                        format!("can't negate a value of type `{t}`"),
                    )
                    .with_primary(span, "`-` needs an `Int` or a `Float`"),
                );
                Type::Error
            }
            (UnaryOp::Not, _) => {
                let mut diag = Diagnostic::error(
                    codes::BAD_UNARY_OPERAND,
                    format!("`not` needs a `Bool`, found `{t}`"),
                )
                .with_primary(operand.span, format!("this has type `{t}`"));
                if let Type::Optional(_) = t {
                    diag = diag.with_help("to check for `none`, write `x == none`");
                }
                self.error(diag);
                Type::Bool
            }
        }
    }

    fn binary(&mut self, op: BinaryOp, lhs: &Expr, rhs: &Expr, e: &Expr) -> Type {
        use BinaryOp::*;
        match op {
            And | Or => {
                let l = self.expr(lhs, &Expect::Infer);
                // The right side runs only sometimes, so its moves are
                // merged with the state where it didn't run.
                let before = self.flow.clone();
                let r = self.expr(rhs, &Expect::Infer);
                self.flow = std::mem::replace(&mut self.flow, Flow::new()).join(before);
                for (t, side) in [(&l, lhs), (&r, rhs)] {
                    if !matches!(t, Type::Bool | Type::Error | Type::Never) {
                        self.error(
                            Diagnostic::error(
                                codes::BAD_BINARY_OPERANDS,
                                format!("`{}` needs `Bool` on both sides", op.symbol()),
                            )
                            .with_primary(side.span, format!("this has type `{t}`")),
                        );
                    }
                }
                Type::Bool
            }
            Coalesce => {
                let l = self.expr(lhs, &Expect::Infer);
                let before = self.flow.clone();
                let result = match &l {
                    Type::Optional(inner) => {
                        let inner = (**inner).clone();
                        let r = self.expr(rhs, &Expect::Infer);
                        if r.fits(&inner) {
                            inner
                        } else if r.fits(&l) {
                            l.clone()
                        } else {
                            self.mismatch(&r, &inner, rhs);
                            inner
                        }
                    }
                    Type::Error => {
                        self.expr(rhs, &Expect::Infer);
                        Type::Error
                    }
                    other => {
                        let other = other.clone();
                        self.error(
                            Diagnostic::error(
                                codes::NOT_OPTIONAL,
                                format!("`??` needs an optional on the left, found `{other}`"),
                            )
                            .with_primary(
                                lhs.span,
                                "never `none`, so the default would never be used",
                            )
                            .with_help("remove `?? ...`"),
                        );
                        self.expr(rhs, &Expect::Infer);
                        other
                    }
                };
                self.flow = std::mem::replace(&mut self.flow, Flow::new()).join(before);
                result
            }
            Eq | Ne => {
                // Check the side that isn't a bare `none` / `[]` first, so
                // the other side can take its type.
                let lhs_needs_context = matches!(lhs.kind, ExprKind::None)
                    || matches!(&lhs.kind, ExprKind::List(items) if items.is_empty());
                let (first, second) = if lhs_needs_context {
                    (rhs, lhs)
                } else {
                    (lhs, rhs)
                };
                let t1 = self.expr(first, &Expect::Infer);
                let expect_second = match &t1 {
                    Type::Never => Expect::Infer,
                    t => Expect::Type(t.clone()),
                };
                let t2 = self.expr_inner_for_compare(second, &expect_second, &t1);
                if t1 == Type::Error || t2 == Type::Error {
                    return Type::Bool;
                }
                Type::Bool
            }
            Lt | Le | Gt | Ge => {
                let l = self.expr(lhs, &Expect::Infer);
                let r = self.expr(rhs, &Expect::Infer);
                if l.is_error() || r.is_error() {
                    return Type::Bool;
                }
                if l != r {
                    if l.is_numeric() && r.is_numeric() {
                        self.mismatch(&r, &l, rhs);
                    } else {
                        self.error(
                            Diagnostic::error(
                                codes::BAD_BINARY_OPERANDS,
                                format!("can't compare `{l}` with `{r}`"),
                            )
                            .with_primary(e.span, "different types")
                            .with_secondary(lhs.span, format!("`{l}`"))
                            .with_secondary(rhs.span, format!("`{r}`")),
                        );
                    }
                } else if !matches!(l, Type::Int | Type::Float | Type::Char | Type::String) {
                    let mut diag = Diagnostic::error(
                        codes::NOT_ORDERED,
                        format!("`{l}` values can't be ordered with `{}`", op.symbol()),
                    )
                    .with_primary(e.span, "no ordering")
                    .with_note("`<`, `<=`, `>` and `>=` work on Int, Float, Char and String");
                    if let Type::Optional(_) = l {
                        diag = diag.with_help("unwrap the values first with `!` or `??`");
                    }
                    self.error(diag);
                }
                Type::Bool
            }
            Add | Sub | Mul | Div | Rem => {
                let l = self.expr(lhs, &Expect::Infer);
                let r = self.expr(rhs, &Expect::Infer);
                self.arithmetic(op, &l, &r, e.span, rhs)
            }
        }
    }

    /// The second operand of `==` / `!=`: it must fit the first one's type,
    /// or the first must fit it (comparing `Int?` with `Int` works both ways).
    fn expr_inner_for_compare(&mut self, e: &Expr, expect: &Expect, other: &Type) -> Type {
        let needs_context = matches!(e.kind, ExprKind::None)
            || matches!(&e.kind, ExprKind::List(items) if items.is_empty());
        if needs_context {
            if let Expect::Type(t) = expect {
                if !matches!(t, Type::Optional(_) | Type::List(_) | Type::Error)
                    && matches!(e.kind, ExprKind::None)
                {
                    self.error(
                        Diagnostic::error(
                            codes::NOT_OPTIONAL,
                            format!("a value of type `{t}` is never `none`"),
                        )
                        .with_primary(e.span, "this comparison is always false")
                        .with_help("only optional values like `Int?` can be `none`"),
                    );
                    return Type::Error;
                }
            }
            return self.expr(e, expect);
        }
        let t = self.expr(e, &Expect::Infer);
        if !t.fits(other) && !other.fits(&t) {
            self.error(
                Diagnostic::error(
                    codes::MISMATCHED_TYPES,
                    format!("can't compare `{other}` with `{t}`"),
                )
                .with_primary(e.span, format!("this has type `{t}`"))
                .with_note("`==` compares values of the same type"),
            );
        }
        t
    }

    /// Checks `+ - * / %` (also for compound assignment). `rhs` is used to
    /// point at the right operand.
    fn arithmetic(&mut self, op: BinaryOp, l: &Type, r: &Type, span: Span, rhs: &Expr) -> Type {
        match (l, r) {
            (Type::Error, _) | (_, Type::Error) => Type::Error,
            (Type::Int, Type::Int) => Type::Int,
            (Type::Float, Type::Float) => Type::Float,
            (Type::String, Type::String) if op == BinaryOp::Add => Type::String,
            (Type::Int, Type::Float) | (Type::Float, Type::Int) => {
                self.mismatch(r, l, rhs);
                Type::Error
            }
            _ => {
                let mut diag = Diagnostic::error(
                    codes::BAD_BINARY_OPERANDS,
                    format!("can't use `{}` on `{l}` and `{r}`", op.symbol()),
                )
                .with_primary(span, "unsupported operands");
                if op == BinaryOp::Add && (*l == Type::String || *r == Type::String) {
                    diag = diag.with_help(
                        "to build text from other values, use interpolation: `\"{a}{b}\"`",
                    );
                } else if matches!(l, Type::Optional(_)) || matches!(r, Type::Optional(_)) {
                    diag = diag.with_help("unwrap the optional first with `!` or `??`");
                }
                self.error(diag);
                Type::Error
            }
        }
    }

    fn field(&mut self, expr: &Expr, field: &Ident) -> Type {
        // `Shape.Point`
        if let ExprKind::Name(enum_name) = &expr.kind {
            if self.lookup_var(enum_name).is_none() && self.enums.contains_key(enum_name) {
                let found = self.enums[enum_name]
                    .variants
                    .iter()
                    .find(|v| v.name == field.name)
                    .map(|v| v.fields.len());
                match found {
                    Some(0) => {}
                    Some(count) => self.error(
                        Diagnostic::error(
                            codes::VARIANT_ARGUMENTS,
                            format!("`{}` needs its field values", field.name),
                        )
                        .with_primary(field.span, format!("has {count} field{}", plural(count)))
                        .with_help(format!("write `{enum_name}.{}(...)`", field.name)),
                    ),
                    None => self.unknown_variant(enum_name, field),
                }
                return Type::Enum(enum_name.clone());
            }
        }

        let t = self.expr(expr, &Expect::Infer);
        match &t {
            Type::Error => Type::Error,
            Type::Struct(name) => {
                let def = &self.structs[name];
                if let Some(f) = def.fields.iter().find(|f| f.name == field.name) {
                    return f.ty.clone();
                }
                let names: Vec<String> = def.fields.iter().map(|f| f.name.clone()).collect();
                let mut diag = Diagnostic::error(
                    codes::UNKNOWN_FIELD,
                    format!("`{name}` has no field `{}`", field.name),
                )
                .with_primary(field.span, "unknown field");
                if let Some(s) = did_you_mean(&field.name, names.iter().map(String::as_str)) {
                    diag = diag.with_help(format!("did you mean `{s}`?"));
                } else {
                    diag = diag.with_note(format!("its fields are: {}", names.join(", ")));
                }
                self.error(diag);
                Type::Error
            }
            Type::Optional(inner) if matches!(**inner, Type::Struct(_)) => {
                self.error(
                    Diagnostic::error(
                        codes::UNKNOWN_FIELD,
                        format!("can't read `.{}` from a value of type `{t}`", field.name),
                    )
                    .with_primary(expr.span, "this might be `none`")
                    .with_help(format!("unwrap it first: `x!.{}`", field.name)),
                );
                Type::Error
            }
            other => {
                let mut diag = Diagnostic::error(
                    codes::UNKNOWN_FIELD,
                    format!("`{other}` has no field `{}`", field.name),
                )
                .with_primary(field.span, "unknown field");
                if builtins::method(other, &field.name).is_some() {
                    diag = diag.with_help(format!(
                        "`{}` is a method; call it: `.{}()`",
                        field.name, field.name
                    ));
                }
                self.error(diag);
                Type::Error
            }
        }
    }

    fn unknown_variant(&mut self, enum_name: &str, name: &Ident) {
        let names: Vec<String> = self.enums[enum_name]
            .variants
            .iter()
            .map(|v| v.name.clone())
            .collect();
        let mut diag = Diagnostic::error(
            codes::UNKNOWN_NAME,
            format!("enum `{enum_name}` has no variant `{}`", name.name),
        )
        .with_primary(name.span, "unknown variant");
        if let Some(s) = did_you_mean(&name.name, names.iter().map(String::as_str)) {
            diag = diag.with_help(format!("did you mean `{s}`?"));
        } else {
            diag = diag.with_note(format!("its variants are: {}", names.join(", ")));
        }
        self.error(diag);
    }

    fn index(&mut self, expr: &Expr, index: &Expr) -> Type {
        let t = self.expr(expr, &Expect::Infer);
        let i = self.expr(index, &Expect::Infer);
        if !matches!(i, Type::Int | Type::Error | Type::Never) {
            self.error(
                Diagnostic::error(
                    codes::BAD_INDEX,
                    format!("a list index must be an `Int`, found `{i}`"),
                )
                .with_primary(index.span, format!("this has type `{i}`")),
            );
        }
        match t {
            Type::List(elem) => *elem,
            Type::Error => Type::Error,
            other => {
                let mut diag = Diagnostic::error(
                    codes::BAD_INDEX,
                    format!("can't index into a value of type `{other}`"),
                )
                .with_primary(expr.span, format!("this has type `{other}`"));
                diag = match other {
                    Type::String => diag.with_help(
                        "strings can't be indexed yet; use `.chars()` to get a list of characters",
                    ),
                    Type::Optional(_) => {
                        diag.with_help("unwrap the optional first with `!` or `??`")
                    }
                    _ => diag.with_note("only lists can be indexed with `[...]`"),
                };
                self.error(diag);
                Type::Error
            }
        }
    }

    fn if_expr(
        &mut self,
        cond: &Expr,
        then: &Block,
        else_: Option<&Expr>,
        span: Span,
        expect: &Expect,
    ) -> Type {
        self.condition(cond, "if");
        let before = self.flow.clone();
        let then_ty = self.block(then, expect);
        let after_then = std::mem::replace(&mut self.flow, before.clone());

        let Some(else_expr) = else_ else {
            if expect.needs_value() {
                self.error(
                    Diagnostic::error(
                        codes::IF_WITHOUT_ELSE,
                        "this `if` has no `else`, so it has no value when the condition is false",
                    )
                    .with_primary(span, "`if` without `else`")
                    .with_help("add an `else:` branch"),
                );
            }
            self.flow = after_then.join(before);
            return if expect.needs_value() {
                Type::Error
            } else {
                Type::Unit
            };
        };

        let else_expect = match (expect, &then_ty) {
            (Expect::Infer, Type::Never) => Expect::Infer,
            (Expect::Infer, t) => Expect::Type(t.clone()),
            (other, _) => other.clone(),
        };
        let else_ty = self.expr(else_expr, &else_expect);
        self.flow = after_then.join(std::mem::replace(&mut self.flow, Flow::new()));

        match expect {
            _ if then_ty == Type::Never && else_ty == Type::Never => Type::Never,
            Expect::Discard => Type::Unit,
            Expect::Type(t) => t.clone(),
            Expect::Infer if then_ty == Type::Never => else_ty,
            Expect::Infer => then_ty,
        }
    }

    // ==================================================================
    // match
    // ==================================================================

    fn match_expr(&mut self, scrutinee: &Expr, arms: &[MatchArm], expect: &Expect) -> Type {
        let st = self.expr(scrutinee, &Expect::Infer);
        let before = self.flow.clone();
        let mut joined: Option<Flow> = None;
        let mut result: Option<Type> = None;
        let mut all_never = true;

        let mut covered_all = false;
        let mut covered_variants: HashSet<String> = HashSet::new();
        let mut covered_bools: HashSet<bool> = HashSet::new();
        let mut pattern_error = false;

        for arm in arms {
            self.flow = before.clone();
            let arm_ty = self.in_scope(|c| {
                let coverage = c.pattern(&arm.pattern, &st);
                let redundant = covered_all
                    || coverage
                        .variant
                        .as_ref()
                        .is_some_and(|v| covered_variants.contains(v))
                    || coverage.boolean.is_some_and(|b| covered_bools.contains(&b));
                if redundant {
                    c.error(
                        Diagnostic::warning(
                            codes::UNREACHABLE_PATTERN,
                            "this match arm can never run",
                        )
                        .with_primary(arm.pattern.span, "already covered by an earlier arm")
                        .with_help("remove this arm, or move it above the arm that covers it"),
                    );
                }
                covered_all |= coverage.all;
                pattern_error |= coverage.error;
                covered_variants.extend(coverage.variant);
                covered_bools.extend(coverage.boolean);

                let arm_expect = match (expect, &result) {
                    (Expect::Infer, Some(t)) => Expect::Type(t.clone()),
                    (other, _) => other.clone(),
                };
                c.block(&arm.body, &arm_expect)
            });
            if arm_ty != Type::Never {
                all_never = false;
                if result.is_none() && matches!(expect, Expect::Infer) {
                    result = Some(arm_ty);
                }
            }
            let state = std::mem::replace(&mut self.flow, Flow::new());
            joined = Some(match joined {
                None => state,
                Some(j) => j.join(state),
            });
        }
        self.flow = joined.unwrap_or(before);

        if !covered_all && !st.is_error() && !pattern_error {
            self.check_exhaustive(&st, scrutinee, &covered_variants, &covered_bools);
        }

        if all_never && !arms.is_empty() {
            return Type::Never;
        }
        match expect {
            Expect::Discard => Type::Unit,
            Expect::Type(t) => t.clone(),
            Expect::Infer => result.unwrap_or(Type::Never),
        }
    }

    fn check_exhaustive(
        &mut self,
        st: &Type,
        scrutinee: &Expr,
        variants: &HashSet<String>,
        bools: &HashSet<bool>,
    ) {
        let missing: Vec<String> = match st {
            Type::Enum(name) => self.enums[name]
                .variants
                .iter()
                .filter(|v| !variants.contains(&v.name))
                .map(|v| {
                    if v.fields.is_empty() {
                        v.name.clone()
                    } else {
                        format!("{}(..)", v.name)
                    }
                })
                .collect(),
            Type::Bool => [true, false]
                .iter()
                .filter(|b| !bools.contains(b))
                .map(|b| b.to_string())
                .collect(),
            _ => {
                self.error(
                    Diagnostic::error(codes::NON_EXHAUSTIVE_MATCH, format!("this `match` doesn't cover every `{st}`"))
                        .with_primary(scrutinee.span, format!("some `{st}` values match no arm"))
                        .with_help("add a last arm `_: ...` (or `other: ...` to use the value) to cover the rest"),
                );
                return;
            }
        };
        if missing.is_empty() {
            return;
        }
        let list = missing
            .iter()
            .map(|m| format!("`{m}`"))
            .collect::<Vec<_>>()
            .join(", ");
        self.error(
            Diagnostic::error(
                codes::NON_EXHAUSTIVE_MATCH,
                format!("this `match` doesn't cover every `{st}`"),
            )
            .with_primary(scrutinee.span, format!("missing {list}"))
            .with_help(format!(
                "add arm{} for {list}, or a `_: ...` arm to cover the rest",
                plural(missing.len())
            )),
        );
    }

    /// Checks a pattern against the matched type, declaring the variables
    /// it binds. Returns what it covers.
    fn pattern(&mut self, pattern: &Pattern, st: &Type) -> Coverage {
        let all = Coverage {
            all: true,
            ..Coverage::default()
        };
        // A broken pattern covers nothing, and turns off the
        // exhaustiveness check so the mistake isn't reported twice.
        let broken = Coverage {
            error: true,
            ..Coverage::default()
        };
        match &pattern.kind {
            PatternKind::Wildcard => all,
            PatternKind::Name(name) if name.starts_with(|c: char| c.is_uppercase()) => {
                // Capitalized names are variants, never new variables, so a
                // misspelled variant is an error instead of a catch-all.
                match self.variant_of(st, name) {
                    Some(fields) if fields.is_empty() => Coverage {
                        variant: Some(name.clone()),
                        ..Coverage::default()
                    },
                    Some(fields) => {
                        let count = fields.len();
                        let blanks = vec!["_"; count].join(", ");
                        self.error(
                            Diagnostic::error(
                                codes::WRONG_PATTERN_ARITY,
                                format!("`{name}` has {count} field{}", plural(count)),
                            )
                            .with_primary(pattern.span, "fields missing from the pattern")
                            .with_help(format!("write `{name}({blanks})` to ignore them")),
                        );
                        broken
                    }
                    None => {
                        self.not_a_variant(st, name, pattern.span);
                        broken
                    }
                }
            }
            PatternKind::Name(name) => {
                self.declare(name, st.clone(), VarKind::Binding, pattern.span);
                all
            }
            PatternKind::Variant { name, args } => {
                let Some(fields) = self.variant_of(st, &name.name) else {
                    if !st.is_error() {
                        self.not_a_variant(st, &name.name, name.span);
                    }
                    for arg in args {
                        self.pattern(arg, &Type::Error);
                    }
                    return broken;
                };
                if fields.len() != args.len() {
                    let count = fields.len();
                    let blanks = vec!["_"; count].join(", ");
                    let help = if count == 0 {
                        format!("write `{}` without parentheses", name.name)
                    } else {
                        format!("write `{}({blanks})`", name.name)
                    };
                    self.error(
                        Diagnostic::error(
                            codes::WRONG_PATTERN_ARITY,
                            format!(
                                "`{}` has {count} field{}, but the pattern has {}",
                                name.name,
                                plural(count),
                                args.len()
                            ),
                        )
                        .with_primary(pattern.span, "wrong number of fields")
                        .with_help(help),
                    );
                    for arg in args {
                        self.pattern(arg, &Type::Error);
                    }
                    return broken;
                }
                let mut complete = true;
                let mut error = false;
                for (arg, field_ty) in args.iter().zip(&fields) {
                    let sub = self.pattern(arg, field_ty);
                    complete &= sub.all;
                    error |= sub.error;
                }
                Coverage {
                    variant: complete.then(|| name.name.clone()),
                    error,
                    ..Coverage::default()
                }
            }
            PatternKind::Int(_) => self.literal_pattern(pattern, st, Type::Int, None),
            PatternKind::Str(_) => self.literal_pattern(pattern, st, Type::String, None),
            PatternKind::Char(_) => self.literal_pattern(pattern, st, Type::Char, None),
            PatternKind::Bool(b) => self.literal_pattern(pattern, st, Type::Bool, Some(*b)),
        }
    }

    fn literal_pattern(
        &mut self,
        pattern: &Pattern,
        st: &Type,
        ty: Type,
        boolean: Option<bool>,
    ) -> Coverage {
        if *st != ty && !st.is_error() {
            self.error(
                Diagnostic::error(
                    codes::PATTERN_TYPE_MISMATCH,
                    format!("a pattern of type `{ty}` can't match a value of type `{st}`"),
                )
                .with_primary(pattern.span, format!("this pattern has type `{ty}`")),
            );
            return Coverage {
                error: true,
                ..Coverage::default()
            };
        }
        Coverage {
            boolean,
            ..Coverage::default()
        }
    }

    /// The field types of variant `name` if `st` is an enum that has it.
    fn variant_of(&self, st: &Type, name: &str) -> Option<Vec<Type>> {
        let Type::Enum(enum_name) = st else {
            return None;
        };
        self.enums[enum_name]
            .variants
            .iter()
            .find(|v| v.name == name)
            .map(|v| v.fields.iter().map(|(_, t)| t.clone()).collect())
    }

    fn not_a_variant(&mut self, st: &Type, name: &str, span: Span) {
        if st.is_error() {
            return;
        }
        let mut diag = Diagnostic::error(
            codes::PATTERN_TYPE_MISMATCH,
            format!("`{name}` is not a variant of `{st}`"),
        )
        .with_primary(span, "unknown variant");
        if let Type::Enum(enum_name) = st {
            let names: Vec<String> = self.enums[enum_name]
                .variants
                .iter()
                .map(|v| v.name.clone())
                .collect();
            if let Some(s) = did_you_mean(name, names.iter().map(String::as_str)) {
                diag = diag.with_help(format!("did you mean `{s}`?"));
            } else {
                diag = diag.with_note(format!("the variants of `{st}` are: {}", names.join(", ")));
            }
        } else {
            diag = diag.with_note("capitalized names in patterns are enum variants; use a lowercase name to bind the value");
        }
        self.error(diag);
    }

    // ==================================================================
    // Calls
    // ==================================================================

    fn call(&mut self, callee: &Expr, args: &[Arg], span: Span) -> Type {
        match &callee.kind {
            ExprKind::Name(name) if self.lookup_var(name).is_none() => {
                if let Some(sig) = self.fns.get(name).cloned() {
                    return self.user_call(&sig, args, span);
                }
                if self.structs.contains_key(name) {
                    return self.construct_struct(name, args, span);
                }
                if let Some((enum_name, index)) = self.variants.get(name).cloned() {
                    return self.construct_variant(&enum_name, index, args, span);
                }
                if builtins::FUNCTIONS.contains(&name.as_str()) {
                    return self.builtin_call(name, args, span);
                }
                if self.enums.contains_key(name) {
                    self.error(
                        Diagnostic::error(
                            codes::NOT_CALLABLE,
                            format!("`{name}` is an enum; create one of its variants instead"),
                        )
                        .with_primary(callee.span, "can't call an enum")
                        .with_help(format!(
                            "write `{name}.Variant(...)` or just `Variant(...)`"
                        )),
                    );
                } else {
                    self.name_expr(name, callee.span);
                }
                self.args_unchecked(args);
                Type::Error
            }
            _ => self.not_callable(callee, args),
        }
    }

    fn not_callable(&mut self, callee: &Expr, args: &[Arg]) -> Type {
        let t = self.expr(callee, &Expect::Infer);
        if !t.is_error() {
            let mut diag = Diagnostic::error(
                codes::NOT_CALLABLE,
                format!("a value of type `{t}` can't be called"),
            )
            .with_primary(callee.span, "not a function");
            if let ExprKind::Field { field, .. } = &callee.kind {
                diag = diag.with_note(format!(
                    "Phase 1 has no user-defined methods; write `{}(x, ...)` as a function instead",
                    field.name
                ));
            }
            self.error(diag);
        }
        self.args_unchecked(args);
        Type::Error
    }

    /// Checks argument expressions on their own, after the call itself
    /// was found to be wrong, so errors inside them are still reported.
    fn args_unchecked(&mut self, args: &[Arg]) {
        for arg in args {
            self.expr(&arg.value, &Expect::Infer);
        }
    }

    fn user_call(&mut self, sig: &FnSig, args: &[Arg], span: Span) -> Type {
        let name = &sig.name;
        if args.len() != sig.params.len() {
            let expected = sig.params.len();
            self.error(
                Diagnostic::error(
                    codes::WRONG_ARGUMENT_COUNT,
                    format!(
                        "`{name}` takes {expected} argument{} but {} {} given",
                        plural(expected),
                        args.len(),
                        was_were(args.len())
                    ),
                )
                .with_primary(
                    span,
                    format!("expected {expected} argument{}", plural(expected)),
                )
                .with_secondary(sig.name_span, format!("`{name}` is defined here")),
            );
            self.args_unchecked(args);
            return sig.ret.clone();
        }
        if let Some(label) = args.iter().find_map(|a| a.label.as_ref()) {
            self.error(
                Diagnostic::error(
                    codes::NAMED_ARGUMENT_TO_FUNCTION,
                    "function arguments can't be named",
                )
                .with_primary(label.span, "named argument")
                .with_note("named arguments are for creating structs and enum variants")
                .with_help("pass the arguments by position, without `name:`"),
            );
        }

        // Read and take arguments first, then `mut` ones (see the
        // interpreter's two-phase calls).
        for (param, arg) in sig.params.iter().zip(args) {
            match (param.mode, arg.mode) {
                (Mode::Mut, _) => continue,
                (Mode::Read, Mode::Read) => {
                    self.expr(&arg.value, &Expect::Type(param.ty.clone()));
                }
                (Mode::Take, Mode::Take) => {
                    let t = self.take(&arg.value, arg.span, Some(name));
                    if !t.fits(&param.ty) {
                        self.mismatch(&t, &param.ty, &arg.value);
                    }
                }
                (Mode::Take, Mode::Read) => {
                    let t = self.expr(&arg.value, &Expect::Type(param.ty.clone()));
                    if !t.is_copy() && is_place(&arg.value) {
                        let shown = describe_expr(&arg.value);
                        self.error(
                            Diagnostic::error(codes::MISSING_TAKE_MARKER, format!("`{name}` takes ownership of this argument"))
                                .with_primary(arg.value.span, "needs `take`")
                                .with_note(format!("parameter `{}` is declared `take`, so the value moves into `{name}`", param.name))
                                .with_help(format!("write `take {shown}` to move it"))
                                .with_help(format!("or `take {shown}.clone()` to move a copy and keep the original")),
                        );
                    }
                }
                (Mode::Read, Mode::Mut | Mode::Take) | (Mode::Take, Mode::Mut) => {
                    self.wrong_marker(name, param, arg);
                    self.expr(&arg.value, &Expect::Type(param.ty.clone()));
                }
            }
        }
        for (param, arg) in sig.params.iter().zip(args) {
            if param.mode != Mode::Mut {
                continue;
            }
            if arg.mode != Mode::Mut {
                if arg.mode == Mode::Take {
                    self.wrong_marker(name, param, arg);
                } else {
                    let shown = describe_expr(&arg.value);
                    self.error(
                        Diagnostic::error(
                            codes::MISSING_MUT_MARKER,
                            format!("`{name}` changes this argument, so it must be marked `mut`"),
                        )
                        .with_primary(arg.value.span, "needs `mut`")
                        .with_note(format!(
                            "parameter `{}` is declared `mut`; the change is visible to the caller",
                            param.name
                        ))
                        .with_help(format!("write `mut {shown}`")),
                    );
                }
            }
            self.mut_arg(&arg.value, &param.ty);
        }
        self.check_borrow_conflicts(args, sig);
        sig.ret.clone()
    }

    fn wrong_marker(&mut self, name: &str, param: &ParamSig, arg: &Arg) {
        let marker = arg.mode.keyword();
        let span = arg.mode_span.unwrap_or(arg.span);
        let (explain, help) = match param.mode {
            Mode::Read => (
                format!("`{name}` only reads parameter `{}`", param.name),
                format!("remove `{marker}`"),
            ),
            Mode::Mut => (
                format!("`{name}` borrows parameter `{}` with `mut`", param.name),
                "write `mut` instead".to_string(),
            ),
            Mode::Take => (
                format!("`{name}` takes ownership of parameter `{}`", param.name),
                "write `take` instead".to_string(),
            ),
        };
        self.error(
            Diagnostic::error(
                codes::WRONG_MODE_MARKER,
                format!("this argument shouldn't be marked `{marker}`"),
            )
            .with_primary(span, format!("`{marker}` doesn't match the parameter"))
            .with_note(explain)
            .with_help(help),
        );
    }

    /// A `mut` argument: must be a place in a variable that may change,
    /// and its type must match exactly (the callee may write to it).
    fn mut_arg(&mut self, value: &Expr, want: &Type) {
        let t = self.expr(value, &Expect::Infer);
        match self.place_root(value) {
            Some(root) => self.check_mutable(root, value.span, Mutation::MutArg),
            None => {
                if !t.is_error() {
                    self.error(
                        Diagnostic::error(
                            codes::MUT_ARG_NOT_PLACE,
                            "only a variable, field or list element can be lent with `mut`",
                        )
                        .with_primary(value.span, "this is a temporary value")
                        .with_help("store it in a `var` first, then pass `mut` that variable"),
                    );
                }
            }
        }
        if t != *want && !t.is_error() && !want.is_error() {
            self.mismatch(&t, want, value);
        }
    }

    /// A variable lent with `mut` can't appear in another argument of the
    /// same call: the callee would see it both changing and not.
    fn check_borrow_conflicts(&mut self, args: &[Arg], sig: &FnSig) {
        for (i, (param, arg)) in sig.params.iter().zip(args).enumerate() {
            if param.mode != Mode::Mut {
                continue;
            }
            let Some(root) = self.place_root(&arg.value) else {
                continue;
            };
            for (j, other) in args.iter().enumerate() {
                if i == j || self.place_root(&other.value) != Some(root) {
                    continue;
                }
                // Report each pair once.
                if j < i && sig.params[j].mode == Mode::Mut {
                    continue;
                }
                let name = self.vars[root].name.clone();
                self.error(
                    Diagnostic::error(
                        codes::CONFLICTING_BORROW,
                        format!("`{name}` is lent with `mut` and also used by another argument"),
                    )
                    .with_primary(other.value.span, "also used here")
                    .with_secondary(arg.span, "lent with `mut` here")
                    .with_note("while a value is lent with `mut`, nothing else may use it")
                    .with_help(format!(
                        "copy what you need first: `let copy = {}`",
                        describe_expr(&other.value)
                    )),
                );
            }
        }
    }

    fn construct_struct(&mut self, name: &str, args: &[Arg], span: Span) -> Type {
        let def = self.structs[name].clone();
        let mut given: HashMap<String, Span> = HashMap::new();
        let mut reported_positional = false;
        for arg in args {
            let Some(label) = &arg.label else {
                if !reported_positional {
                    let example = def
                        .fields
                        .iter()
                        .map(|f| format!("{}: ...", f.name))
                        .collect::<Vec<_>>()
                        .join(", ");
                    self.error(
                        Diagnostic::error(
                            codes::UNNAMED_STRUCT_FIELD,
                            format!("fields of `{name}` must be named"),
                        )
                        .with_primary(arg.span, "unnamed field value")
                        .with_help(format!("write `{name}({example})`")),
                    );
                    reported_positional = true;
                }
                self.expr(&arg.value, &Expect::Infer);
                continue;
            };
            let Some(field) = def.fields.iter().find(|f| f.name == label.name) else {
                let names: Vec<&str> = def.fields.iter().map(|f| f.name.as_str()).collect();
                let mut diag = Diagnostic::error(
                    codes::UNKNOWN_FIELD,
                    format!("`{name}` has no field `{}`", label.name),
                )
                .with_primary(label.span, "unknown field");
                if let Some(s) = did_you_mean(&label.name, names.iter().copied()) {
                    diag = diag.with_help(format!("did you mean `{s}`?"));
                } else {
                    diag = diag.with_note(format!("its fields are: {}", names.join(", ")));
                }
                self.error(diag);
                self.expr(&arg.value, &Expect::Infer);
                continue;
            };
            if let Some(first) = given.get(&label.name) {
                let first = *first;
                self.error(
                    Diagnostic::error(
                        codes::DUPLICATE_FIELD,
                        format!("field `{}` is given twice", label.name),
                    )
                    .with_primary(label.span, "second value")
                    .with_secondary(first, "first value"),
                );
            }
            given.insert(label.name.clone(), label.span);
            self.field_value(arg, &field.ty.clone());
        }
        if !reported_positional {
            let missing: Vec<&str> = def
                .fields
                .iter()
                .filter(|f| !f.has_default && !given.contains_key(&f.name))
                .map(|f| f.name.as_str())
                .collect();
            if !missing.is_empty() {
                let list = missing
                    .iter()
                    .map(|m| format!("`{m}`"))
                    .collect::<Vec<_>>()
                    .join(", ");
                self.error(
                    Diagnostic::error(
                        codes::MISSING_FIELD,
                        format!("missing field{} {list} in `{name}`", plural(missing.len())),
                    )
                    .with_primary(span, format!("{list} not given"))
                    .with_help(format!(
                        "add {}",
                        missing
                            .iter()
                            .map(|m| format!("`{m}: ...`"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )),
                );
            }
        }
        Type::Struct(name.to_string())
    }

    /// A value stored into a struct or variant field: `take` moves it in,
    /// a plain value is copied, and `mut` makes no sense.
    fn field_value(&mut self, arg: &Arg, ty: &Type) {
        match arg.mode {
            Mode::Read => {
                self.expr(&arg.value, &Expect::Type(ty.clone()));
            }
            Mode::Take => {
                let t = self.take(&arg.value, arg.span, None);
                if !t.fits(ty) {
                    self.mismatch(&t, ty, &arg.value);
                }
            }
            Mode::Mut => {
                self.error(
                    Diagnostic::error(
                        codes::WRONG_MODE_MARKER,
                        "a field value can't be lent with `mut`",
                    )
                    .with_primary(arg.mode_span.unwrap_or(arg.span), "`mut` not allowed here")
                    .with_note("a struct or variant holds its own values, not borrows")
                    .with_help(
                        "remove `mut` to store a copy, or write `take` to move the value in",
                    ),
                );
                self.expr(&arg.value, &Expect::Type(ty.clone()));
            }
        }
    }

    fn construct_variant(
        &mut self,
        enum_name: &str,
        index: usize,
        args: &[Arg],
        span: Span,
    ) -> Type {
        let variant = self.enums[enum_name].variants[index].clone();
        let result = Type::Enum(enum_name.to_string());
        if variant.fields.is_empty() {
            self.error(
                Diagnostic::error(
                    codes::VARIANT_ARGUMENTS,
                    format!("`{}` has no fields", variant.name),
                )
                .with_primary(span, "called with parentheses")
                .with_help(format!("write just `{}`", variant.name)),
            );
            self.args_unchecked(args);
            return result;
        }
        let named = args.iter().any(|a| a.label.is_some());
        if !named {
            if args.len() != variant.fields.len() {
                let expected = variant.fields.len();
                self.error(
                    Diagnostic::error(
                        codes::WRONG_ARGUMENT_COUNT,
                        format!(
                            "`{}` has {expected} field{} but {} {} given",
                            variant.name,
                            plural(expected),
                            args.len(),
                            was_were(args.len())
                        ),
                    )
                    .with_primary(
                        span,
                        format!("expected {expected} value{}", plural(expected)),
                    )
                    .with_secondary(variant.span, "variant declared here"),
                );
                self.args_unchecked(args);
                return result;
            }
            for (arg, (_, ty)) in args.iter().zip(&variant.fields) {
                self.field_value(arg, ty);
            }
            return result;
        }

        let mut given: HashSet<String> = HashSet::new();
        for arg in args {
            let field = arg.label.as_ref().and_then(|label| {
                variant
                    .fields
                    .iter()
                    .find(|(n, _)| n.as_deref() == Some(&label.name))
                    .map(|(_, t)| (label, t.clone()))
            });
            match field {
                Some((label, ty)) => {
                    if !given.insert(label.name.clone()) {
                        self.error(
                            Diagnostic::error(
                                codes::DUPLICATE_FIELD,
                                format!("field `{}` is given twice", label.name),
                            )
                            .with_primary(label.span, "second value"),
                        );
                    }
                    self.field_value(arg, &ty);
                }
                None => {
                    let shown = arg.label.as_ref().map_or_else(
                        || "an unnamed value".to_string(),
                        |l| format!("field `{}`", l.name),
                    );
                    self.error(
                        Diagnostic::error(codes::UNKNOWN_FIELD, format!("`{}` has no {shown}", variant.name))
                            .with_primary(arg.span, "doesn't match a field")
                            .with_note("when some values are named, all of them must use the variant's field names"),
                    );
                    self.expr(&arg.value, &Expect::Infer);
                }
            }
        }
        let missing: Vec<String> = variant
            .fields
            .iter()
            .filter_map(|(n, _)| n.clone())
            .filter(|n| !given.contains(n))
            .collect();
        if !missing.is_empty() {
            let list = missing
                .iter()
                .map(|m| format!("`{m}`"))
                .collect::<Vec<_>>()
                .join(", ");
            self.error(
                Diagnostic::error(
                    codes::MISSING_FIELD,
                    format!(
                        "missing field{} {list} in `{}`",
                        plural(missing.len()),
                        variant.name
                    ),
                )
                .with_primary(span, format!("{list} not given")),
            );
        }
        result
    }

    fn builtin_call(&mut self, name: &str, args: &[Arg], span: Span) -> Type {
        for arg in args {
            if let Some(mode_span) = arg.mode_span {
                self.error(
                    Diagnostic::error(
                        codes::WRONG_MODE_MARKER,
                        format!("`{name}` only reads its arguments"),
                    )
                    .with_primary(mode_span, format!("remove `{}`", arg.mode.keyword())),
                );
            }
            if let Some(label) = &arg.label {
                self.error(
                    Diagnostic::error(
                        codes::NAMED_ARGUMENT_TO_FUNCTION,
                        "function arguments can't be named",
                    )
                    .with_primary(label.span, "named argument")
                    .with_help(format!("remove `{}:`", label.name)),
                );
            }
        }
        let (min, max) = match name {
            "print" => (0, 1),
            "args" => (0, 0),
            _ => (1, 1),
        };
        if args.len() < min || args.len() > max {
            let expected = if min == max {
                format!("{min}")
            } else {
                format!("{min} or {max}")
            };
            self.error(
                Diagnostic::error(
                    codes::WRONG_ARGUMENT_COUNT,
                    format!(
                        "`{name}` takes {expected} argument{} but {} {} given",
                        plural(max),
                        args.len(),
                        was_were(args.len())
                    ),
                )
                .with_primary(span, format!("expected {expected}")),
            );
            self.args_unchecked(args);
            return match name {
                "print" => Type::Unit,
                "len" | "Int" => Type::Int,
                "Float" => Type::Float,
                "String" => Type::String,
                "read_file" => Type::optional(Type::String),
                "args" => Type::list(Type::String),
                _ => Type::Never,
            };
        }
        let arg = args.first().map(|a| &a.value);
        match name {
            "print" => {
                if let Some(arg) = arg {
                    self.expr(arg, &Expect::Infer);
                }
                Type::Unit
            }
            "args" => Type::list(Type::String),
            "read_file" => {
                self.expr(arg.unwrap(), &Expect::Type(Type::String));
                Type::optional(Type::String)
            }
            "panic" => {
                self.expr(arg.unwrap(), &Expect::Type(Type::String));
                Type::Never
            }
            "String" => {
                self.expr(arg.unwrap(), &Expect::Infer);
                Type::String
            }
            "len" => {
                let arg = arg.unwrap();
                let t = self.expr(arg, &Expect::Infer);
                if !matches!(t, Type::List(_) | Type::String | Type::Error) {
                    self.error(
                        Diagnostic::error(
                            codes::MISMATCHED_TYPES,
                            format!("`len` needs a list or a string, found `{t}`"),
                        )
                        .with_primary(arg.span, format!("this has type `{t}`")),
                    );
                }
                Type::Int
            }
            "Int" | "Float" => {
                let arg = arg.unwrap();
                let t = self.expr(arg, &Expect::Infer);
                let ok = match name {
                    "Int" => matches!(t, Type::Int | Type::Float | Type::Char | Type::Error),
                    _ => matches!(t, Type::Int | Type::Float | Type::Error),
                };
                if !ok {
                    let mut diag = Diagnostic::error(
                        codes::INVALID_CONVERSION,
                        format!("can't convert `{t}` to `{name}`"),
                    )
                    .with_primary(arg.span, format!("this has type `{t}`"));
                    if t == Type::String {
                        let method = if name == "Int" { "to_int" } else { "to_float" };
                        diag = diag.with_help(format!("to parse text, use `.{method}()`, which gives `none` if the text isn't a number"));
                    } else if let Type::Optional(_) = t {
                        diag = diag.with_help("unwrap the optional first with `!` or `??`");
                    }
                    self.error(diag);
                }
                if name == "Int" {
                    Type::Int
                } else {
                    Type::Float
                }
            }
            _ => unreachable!("unknown built-in `{name}`"),
        }
    }

    fn method_call(&mut self, receiver: &Expr, method: &Ident, args: &[Arg], span: Span) -> Type {
        // `Shape.Circle(1.0)` parses as a method call on the enum's name.
        if let ExprKind::Name(enum_name) = &receiver.kind {
            if self.lookup_var(enum_name).is_none() && self.enums.contains_key(enum_name) {
                let index = self.enums[enum_name]
                    .variants
                    .iter()
                    .position(|v| v.name == method.name);
                return match index {
                    Some(i) => self.construct_variant(enum_name, i, args, span),
                    None => {
                        self.unknown_variant(enum_name, method);
                        self.args_unchecked(args);
                        Type::Enum(enum_name.clone())
                    }
                };
            }
        }
        let rt = self.expr(receiver, &Expect::Infer);
        if rt.is_error() {
            self.args_unchecked(args);
            return Type::Error;
        }
        let name = method.name.as_str();
        let Some(sig) = builtins::method(&rt, name) else {
            let mut diag = Diagnostic::error(
                codes::UNKNOWN_METHOD,
                format!("`{rt}` has no method `{name}`"),
            )
            .with_primary(method.span, "unknown method");
            match &rt {
                Type::Optional(inner) if builtins::method(inner, name).is_some() => {
                    diag = diag
                        .with_note("this value might be `none`")
                        .with_help(format!(
                            "unwrap it first: `x!.{name}(...)` or `(x ?? default).{name}(...)`"
                        ));
                }
                Type::Struct(_) | Type::Enum(_) => {
                    diag = diag.with_note(format!(
                        "Phase 1 has no user-defined methods; write a function and call it as `{name}(x, ...)`"
                    ));
                }
                _ => {
                    let names = builtins::method_names(&rt);
                    if let Some(s) = did_you_mean(name, names.iter().copied()) {
                        diag = diag.with_help(format!("did you mean `{s}`?"));
                    } else if !names.is_empty() {
                        diag = diag.with_note(format!("`{rt}` has: {}", names.join(", ")));
                    }
                }
            }
            self.error(diag);
            self.args_unchecked(args);
            return Type::Error;
        };

        if args.len() != sig.params.len() {
            let expected = sig.params.len();
            self.error(
                Diagnostic::error(
                    codes::WRONG_ARGUMENT_COUNT,
                    format!(
                        "`{name}` takes {expected} argument{} but {} {} given",
                        plural(expected),
                        args.len(),
                        was_were(args.len())
                    ),
                )
                .with_primary(method.span, format!("expected {expected}")),
            );
            self.args_unchecked(args);
            return sig.ret;
        }
        for (arg, ty) in args.iter().zip(&sig.params) {
            if arg.label.is_some() {
                self.error(
                    Diagnostic::error(
                        codes::NAMED_ARGUMENT_TO_FUNCTION,
                        "method arguments can't be named",
                    )
                    .with_primary(arg.span, "named argument"),
                );
            }
            match arg.mode {
                // `xs.push(take item)` moves the item into the list.
                Mode::Take if sig.mutates => {
                    let t = self.take(&arg.value, arg.span, None);
                    if !t.fits(ty) {
                        self.mismatch(&t, ty, &arg.value);
                    }
                }
                Mode::Read => {
                    self.expr(&arg.value, &Expect::Type(ty.clone()));
                }
                _ => {
                    self.error(
                        Diagnostic::error(
                            codes::WRONG_MODE_MARKER,
                            format!("`{name}` only reads its arguments"),
                        )
                        .with_primary(
                            arg.mode_span.unwrap_or(arg.span),
                            format!("remove `{}`", arg.mode.keyword()),
                        ),
                    );
                    self.expr(&arg.value, &Expect::Type(ty.clone()));
                }
            }
        }
        if sig.mutates {
            match self.place_root(receiver) {
                Some(root) => self.check_mutable(root, receiver.span, Mutation::Method(name)),
                None => self.error(
                    Diagnostic::error(
                        codes::MUTATING_IMMUTABLE,
                        format!("`{name}` changes the list, so it needs a variable"),
                    )
                    .with_primary(receiver.span, "a temporary list")
                    .with_help("store the list in a `var` first"),
                ),
            }
            if name == "pop"
                && matches!(sig.ret, Type::Optional(ref inner) if matches!(**inner, Type::Optional(_)))
            {
                self.error(
                    Diagnostic::error(
                        codes::NESTED_OPTIONAL,
                        "`pop` on a list of optionals isn't supported",
                    )
                    .with_primary(method.span, "would return an optional of an optional")
                    .with_help(
                        "check `len` first and read the last element with `xs[len(xs) - 1]`",
                    ),
                );
            }
        }
        sig.ret
    }

    // ==================================================================
    // Moves and mutability
    // ==================================================================

    /// The variable at the root of a place expression: `x`, `x.field`,
    /// `x[i].field`. `None` for anything else.
    fn place_root(&self, e: &Expr) -> Option<VarId> {
        match &e.kind {
            ExprKind::Name(name) => self.lookup_var(name),
            ExprKind::Field { expr, .. } | ExprKind::Index { expr, .. } => self.place_root(expr),
            _ => None,
        }
    }

    /// Reports a use of a variable whose value was moved away.
    fn use_var(&mut self, id: VarId, span: Span) {
        let Some(site) = self.flow.moved.remove(&id) else {
            return;
        };
        // Removing the move means later uses aren't reported again.
        let name = self.vars[id].name.clone();
        let moved_label = match &site.into {
            Some(f) => format!("ownership moved into `{f}` here"),
            None => "moved here".to_string(),
        };
        let mut diag = Diagnostic::error(
            codes::USE_AFTER_MOVE,
            format!("`{name}` was moved and can't be used here"),
        )
        .with_secondary(site.span, moved_label)
        .with_primary(span, "used after the move");
        if let Some(f) = &site.into {
            diag = diag.with_help(format!("pass a read borrow instead: `{f}({name})`"));
        }
        diag = diag.with_help(format!(
            "or move a copy and keep the original: `take {name}.clone()`"
        ));
        self.error(diag);
    }

    /// `take expr`. Moves a variable; a copy type is copied instead.
    /// `into` names the function receiving it, for messages.
    fn take(&mut self, inner: &Expr, span: Span, into: Option<&str>) -> Type {
        match &inner.kind {
            ExprKind::Name(name) if self.lookup_var(name).is_some() => {
                let id = self.lookup_var(name).unwrap();
                self.use_var(id, inner.span);
                let var = self.vars[id].clone();
                if var.ty.is_copy() {
                    return var.ty;
                }
                match var.kind {
                    VarKind::Param(mode @ (Mode::Read | Mode::Mut)) => {
                        let how = if mode == Mode::Read { "read" } else { "`mut`" };
                        self.error(
                            Diagnostic::error(codes::MOVE_OUT_OF_BORROW, format!("can't move `{name}`: it's only borrowed"))
                                .with_primary(inner.span, "can't be moved")
                                .with_secondary(var.span, format!("`{name}` is a {how} parameter, so the caller still owns it"))
                                .with_help(format!("move a copy instead: `take {name}.clone()`"))
                                .with_help(format!("or declare the parameter as `{name}: take {}` to receive ownership", var.ty)),
                        );
                    }
                    _ => {
                        self.flow.moved.insert(
                            id,
                            MoveSite {
                                span,
                                into: into.map(str::to_string),
                            },
                        );
                    }
                }
                var.ty
            }
            ExprKind::Field { .. } | ExprKind::Index { .. } => {
                let t = self.expr(inner, &Expect::Infer);
                if !t.is_copy() {
                    let shown = describe_expr(inner);
                    self.error(
                        Diagnostic::error(
                            codes::MOVE_OUT_OF_FIELD,
                            "can't move a value out of a field or list element",
                        )
                        .with_primary(inner.span, "part of a larger value")
                        .with_note("moving it would leave a hole in the struct or list")
                        .with_help(format!("move a copy instead: `take {shown}.clone()`")),
                    );
                }
                t
            }
            // A temporary (like a call result) is already owned.
            _ => self.expr(inner, &Expect::Infer),
        }
    }

    /// Reports changing a variable that isn't allowed to change.
    fn check_mutable(&mut self, id: VarId, span: Span, mutation: Mutation) {
        let var = self.vars[id].clone();
        let name = &var.name;
        let (code, message) = match (var.kind, mutation) {
            (VarKind::Var | VarKind::Param(Mode::Mut | Mode::Take), _) => return,
            (VarKind::Param(Mode::Read), Mutation::Assign) => (
                codes::ASSIGN_TO_READ_PARAM,
                format!("can't change `{name}`: it's a read-only parameter"),
            ),
            (_, Mutation::Assign) => (
                codes::ASSIGN_TO_IMMUTABLE,
                format!("can't change `{name}`: it was declared with `let`"),
            ),
            (_, Mutation::MutArg) => (
                codes::MUT_ARG_IMMUTABLE,
                format!("can't lend `{name}` with `mut`: it can't change"),
            ),
            (_, Mutation::Method(method)) => (
                codes::MUTATING_IMMUTABLE,
                format!("can't call `{method}` on `{name}`: it can't change"),
            ),
        };
        let (label, help) = match var.kind {
            VarKind::Param(_) => (
                format!("`{name}` is a read parameter"),
                format!(
                    "to change the caller's value, declare it `{name}: mut {}`",
                    var.ty
                ),
            ),
            VarKind::LoopVar => (
                format!("`{name}` is a loop variable"),
                "loop variables can't change; copy it into a `var` first".to_string(),
            ),
            VarKind::Binding => (
                format!("`{name}` is bound by a pattern"),
                "copy it into a `var` first".to_string(),
            ),
            _ => (
                "declared with `let` here".to_string(),
                format!("declare it with `var` instead: `var {name} = ...`"),
            ),
        };
        let primary = match mutation {
            Mutation::MutArg => "lent with `mut` here",
            _ => "changed here",
        };
        self.error(
            Diagnostic::error(code, message)
                .with_primary(span, primary)
                .with_secondary(var.span, label)
                .with_help(help),
        );
    }
}

// ======================================================================
// Helpers
// ======================================================================

/// The struct or enum a field type stores inline (not through a list).
fn direct_type_name(t: &Type) -> Option<String> {
    match t {
        Type::Struct(n) | Type::Enum(n) => Some(n.clone()),
        Type::Optional(inner) => direct_type_name(inner),
        _ => None,
    }
}

fn is_place(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::Name(_) => true,
        ExprKind::Field { expr, .. } | ExprKind::Index { expr, .. } => is_place(expr),
        _ => false,
    }
}

/// A short rendering of a simple expression for help messages.
fn describe_expr(e: &Expr) -> String {
    match &e.kind {
        ExprKind::Name(n) => n.clone(),
        ExprKind::Field { expr, field } => format!("{}.{}", describe_expr(expr), field.name),
        ExprKind::Index { expr, .. } => format!("{}[...]", describe_expr(expr)),
        _ => "...".to_string(),
    }
}

fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}

fn was_were(n: usize) -> &'static str {
    if n == 1 {
        "was"
    } else {
        "were"
    }
}

/// The closest candidate within a small edit distance, for "did you mean".
fn did_you_mean<'a>(name: &str, candidates: impl Iterator<Item = &'a str>) -> Option<String> {
    let limit = (name.chars().count() / 3).max(1);
    candidates
        .filter(|c| *c != name)
        .map(|c| (edit_distance(name, c), c))
        .filter(|(d, _)| *d <= limit)
        .min_by_key(|(d, c)| (*d, c.to_string()))
        .map(|(_, c)| c.to_string())
}

/// Edit distance, case-insensitive, where swapping two neighboring
/// letters counts as one edit (`Itn` → `Int`).
fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.to_lowercase().chars().collect();
    let b: Vec<char> = b.to_lowercase().chars().collect();
    let mut d = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, cell) in d[0].iter_mut().enumerate() {
        *cell = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            d[i][j] = (d[i - 1][j] + 1)
                .min(d[i][j - 1] + 1)
                .min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                d[i][j] = d[i][j].min(d[i - 2][j - 2] + 1);
            }
        }
    }
    d[a.len()][b.len()]
}
