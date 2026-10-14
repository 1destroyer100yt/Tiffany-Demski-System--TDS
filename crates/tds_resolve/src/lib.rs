//! Modules and imports: loads every file of a program and merges them into
//! one module for the later stages.
//!
//! # Modules
//!
//! One file is one module. The root file (usually `src/main.tds`) is the
//! root module; `use motors.types` refers to the file `motors/types.tds`
//! next to it. Items are private to their module unless declared `pub`.
//!
//! ```tds
//! use motors.types                 // the module, used as `types.Motor`
//! use motors.types.{Motor, Shape}  // items, used as `Motor`, `Shape`
//! use motors.types.Motor           // a single item
//! ```
//!
//! Importing an enum also makes its variants usable without the enum name,
//! as in the brief's `match` examples.
//!
//! # Merging
//!
//! After resolution, every module's items are renamed to unique names
//! (`motors.types.Motor`; the root module keeps plain names), every
//! reference is rewritten to use those names, and all items are put in a
//! single [`Module`]. The type checker and the backends then work on one
//! module with unique names and don't need to know about files. Values
//! print with the short name, because printing strips the module path.

use std::collections::HashMap;
use std::path::PathBuf;

use tds_ast::*;
use tds_diagnostics::{codes, Diagnostic, SourceMap, Span};

/// Reads module files. The driver reads from disk; tests use memory.
pub trait FileLoader {
    /// Loads the module file at `rel_path` (like `motors/types.tds`),
    /// relative to the root module's directory. Returns the name to show
    /// in messages and the text.
    fn load(&self, rel_path: &str) -> Option<(String, String)>;
}

/// Loads modules from a directory on disk.
pub struct DirLoader {
    pub dir: PathBuf,
    /// Put in front of module paths in messages, like `src/`, so names are
    /// shown the way the user wrote the main file's path.
    pub display_prefix: String,
}

impl FileLoader for DirLoader {
    fn load(&self, rel_path: &str) -> Option<(String, String)> {
        let text = std::fs::read_to_string(self.dir.join(rel_path)).ok()?;
        Some((format!("{}{rel_path}", self.display_prefix), text))
    }
}

/// Loads modules from an in-memory list of `(path, text)`.
pub struct MemoryLoader(pub Vec<(String, String)>);

impl FileLoader for MemoryLoader {
    fn load(&self, rel_path: &str) -> Option<(String, String)> {
        self.0
            .iter()
            .find(|(p, _)| p == rel_path)
            .map(|(p, t)| (p.clone(), t.clone()))
    }
}

/// A whole program: all modules merged into one, and its source files.
pub struct Program {
    pub module: Module,
    pub sources: SourceMap,
}

/// Loads the root module and everything it imports, directly or not.
/// Always returns a program; it may be incomplete if there are errors.
pub fn load_program(
    root_name: &str,
    root_text: &str,
    loader: &dyn FileLoader,
) -> (Program, Vec<Diagnostic>) {
    let mut sources = SourceMap::new();
    let mut diagnostics = Vec::new();
    let base = sources.add(root_name, root_text);
    let (root, errors) = tds_parser::parse_module_at(root_text, base);
    diagnostics.extend(errors);

    let mut modules = vec![ModuleData {
        path: Vec::new(),
        module: root,
    }];
    let mut index: HashMap<String, usize> = HashMap::new();
    index.insert(String::new(), 0);

    // Load modules breadth-first, and decide what each `use` refers to.
    let mut imports: Vec<Vec<Import>> = Vec::new();
    let mut next = 0;
    while next < modules.len() {
        let uses = modules[next].module.uses.clone();
        let mut module_imports = Vec::new();
        for u in &uses {
            module_imports.extend(classify_use(
                u,
                &mut modules,
                &mut index,
                &mut sources,
                loader,
                &mut diagnostics,
            ));
        }
        imports.push(module_imports);
        next += 1;
    }

    let exports: Vec<Exports> = modules
        .iter()
        .map(|m| Exports::of(&m.path, &m.module))
        .collect();
    let enum_prefixes: HashMap<String, String> = exports
        .iter()
        .flat_map(|e| e.items.values())
        .filter(|item| item.kind == ItemKind::Enum)
        .map(|item| (item.full.clone(), item.prefix.clone()))
        .collect();

    let mut merged = Module::default();
    for (i, data) in modules.into_iter().enumerate() {
        let scope = build_scope(i, &exports, &imports[i], &mut diagnostics);
        let mut module = data.module;
        let prefix = prefix_of(&data.path);
        let mut rewriter = Rewriter {
            scope: &scope,
            exports: &exports,
            enum_prefixes: &enum_prefixes,
            locals: Vec::new(),
            diagnostics: &mut diagnostics,
        };
        for item in &mut module.items {
            rewriter.item(item, &prefix);
        }
        merged.items.extend(module.items);
    }
    (
        Program {
            module: merged,
            sources,
        },
        diagnostics,
    )
}

/// A parsed module and its dotted path (empty for the root).
struct ModuleData {
    path: Vec<String>,
    module: Module,
}

/// What one `use` brings into scope.
enum Import {
    /// `use a.b`: the module, under its last name.
    Module { alias: Ident, module: usize },
    /// `use a.b.X` or `use a.b.{X}`: an item.
    Item { name: Ident, module: usize },
}

fn prefix_of(path: &[String]) -> String {
    if path.is_empty() {
        String::new()
    } else {
        format!("{}.", path.join("."))
    }
}

fn file_for(path: &[String]) -> String {
    format!("{}.tds", path.join("/"))
}

/// Loads (if needed) the module a dotted path names. Returns its index,
/// or `None` if there is no such file.
fn load_module(
    path: &[String],
    modules: &mut Vec<ModuleData>,
    index: &mut HashMap<String, usize>,
    sources: &mut SourceMap,
    loader: &dyn FileLoader,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<usize> {
    let key = path.join(".");
    if let Some(&i) = index.get(&key) {
        return Some(i);
    }
    let (name, text) = loader.load(&file_for(path))?;
    let base = sources.add(&name, &text);
    let (module, errors) = tds_parser::parse_module_at(&text, base);
    diagnostics.extend(errors);
    modules.push(ModuleData {
        path: path.to_vec(),
        module,
    });
    index.insert(key, modules.len() - 1);
    Some(modules.len() - 1)
}

fn classify_use(
    u: &UseDecl,
    modules: &mut Vec<ModuleData>,
    index: &mut HashMap<String, usize>,
    sources: &mut SourceMap,
    loader: &dyn FileLoader,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<Import> {
    let path: Vec<String> = u.path.iter().map(|i| i.name.clone()).collect();
    if path[0] == "std" || path[0] == "core" || path[0] == "alloc" {
        diagnostics.push(
            Diagnostic::error(
                codes::NOT_SUPPORTED_YET,
                "the standard library isn't available yet",
            )
            .with_primary(u.span, "can't import from the standard library")
            .with_note(
                "`core`, `alloc` and `std` arrive in Phase 4; for now, use the built-in functions",
            ),
        );
        return Vec::new();
    }

    if let Some(names) = &u.names {
        // `use a.b.{X, Y}`: `a.b` must be a module.
        let Some(module) = load_module(&path, modules, index, sources, loader, diagnostics) else {
            diagnostics.push(unknown_module(u, &path));
            return Vec::new();
        };
        return names
            .iter()
            .map(|n| Import::Item {
                name: n.clone(),
                module,
            })
            .collect();
    }

    // `use a.b.c`: the module `a.b.c` if there is one, else item `c` of
    // module `a.b`.
    if let Some(module) = load_module(&path, modules, index, sources, loader, diagnostics) {
        return vec![Import::Module {
            alias: u.path.last().unwrap().clone(),
            module,
        }];
    }
    if path.len() >= 2 {
        if let Some(module) = load_module(
            &path[..path.len() - 1],
            modules,
            index,
            sources,
            loader,
            diagnostics,
        ) {
            return vec![Import::Item {
                name: u.path.last().unwrap().clone(),
                module,
            }];
        }
    }
    diagnostics.push(unknown_module(u, &path));
    Vec::new()
}

fn unknown_module(u: &UseDecl, path: &[String]) -> Diagnostic {
    Diagnostic::error(
        codes::UNKNOWN_MODULE,
        format!("can't find module `{}`", path.join(".")),
    )
    .with_primary(u.span, "no such module")
    .with_note(format!(
        "module `{}` would be the file `{}`, next to the main file",
        path.join("."),
        file_for(path)
    ))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ItemKind {
    Fn,
    Struct,
    Enum,
}

#[derive(Debug, Clone)]
struct Export {
    kind: ItemKind,
    /// The unique name after merging.
    full: String,
    /// The module prefix, like `motors.types.`.
    prefix: String,
    is_pub: bool,
    span: Span,
    /// For enums: (short name, full name) of each variant.
    variants: Vec<(String, String)>,
}

/// The items a module declares.
struct Exports {
    items: HashMap<String, Export>,
}

impl Exports {
    fn of(path: &[String], module: &Module) -> Exports {
        let prefix = prefix_of(path);
        let mut items = HashMap::new();
        for item in &module.items {
            let (kind, variants) = match item {
                Item::Fn(_) => (ItemKind::Fn, Vec::new()),
                Item::Struct(_) => (ItemKind::Struct, Vec::new()),
                Item::Enum(e) => (
                    ItemKind::Enum,
                    e.variants
                        .iter()
                        .map(|v| (v.name.name.clone(), format!("{prefix}{}", v.name.name)))
                        .collect(),
                ),
            };
            let name = &item.name().name;
            // Duplicates are reported by the checker after merging.
            items.entry(name.clone()).or_insert(Export {
                kind,
                full: format!("{prefix}{name}"),
                prefix: prefix.clone(),
                is_pub: item.is_pub(),
                span: item.name().span,
                variants,
            });
        }
        Exports { items }
    }
}

/// What a name refers to inside one module.
#[derive(Debug, Clone)]
enum Binding {
    /// A function, struct, enum or variant, by unique name.
    Item(String),
    /// An imported module.
    Module(usize),
}

struct Scope {
    names: HashMap<String, (Binding, Span)>,
}

fn build_scope(
    module: usize,
    exports: &[Exports],
    imports: &[Import],
    diagnostics: &mut Vec<Diagnostic>,
) -> Scope {
    let mut scope = Scope {
        names: HashMap::new(),
    };
    for (name, export) in &exports[module].items {
        scope.names.insert(
            name.clone(),
            (Binding::Item(export.full.clone()), export.span),
        );
        for (short, full) in &export.variants {
            scope
                .names
                .entry(short.clone())
                .or_insert((Binding::Item(full.clone()), export.span));
        }
    }

    for import in imports {
        match import {
            Import::Module { alias, module: m } => {
                bind(
                    &mut scope,
                    &alias.name,
                    Binding::Module(*m),
                    alias.span,
                    diagnostics,
                );
            }
            Import::Item { name, module: m } => {
                let Some(export) = exports[*m].items.get(&name.name) else {
                    let mut diag = Diagnostic::error(
                        codes::UNKNOWN_IMPORT,
                        format!("there's no `{}` to import here", name.name),
                    )
                    .with_primary(name.span, "not declared in that module");
                    let names: Vec<&str> = exports[*m].items.keys().map(String::as_str).collect();
                    if !names.is_empty() {
                        let mut names = names;
                        names.sort();
                        diag = diag.with_note(format!("the module declares: {}", names.join(", ")));
                    }
                    diagnostics.push(diag);
                    continue;
                };
                if !export.is_pub {
                    diagnostics.push(private_item(&name.name, name.span, export.span));
                }
                bind(
                    &mut scope,
                    &name.name,
                    Binding::Item(export.full.clone()),
                    name.span,
                    diagnostics,
                );
                for (short, full) in &export.variants {
                    bind(
                        &mut scope,
                        short,
                        Binding::Item(full.clone()),
                        name.span,
                        diagnostics,
                    );
                }
            }
        }
    }
    scope
}

fn bind(
    scope: &mut Scope,
    name: &str,
    binding: Binding,
    span: Span,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if let Some((existing, first)) = scope.names.get(name) {
        let same = matches!((existing, &binding), (Binding::Item(a), Binding::Item(b)) if a == b)
            || matches!((existing, &binding), (Binding::Module(a), Binding::Module(b)) if a == b);
        if !same {
            diagnostics.push(
                Diagnostic::error(
                    codes::IMPORT_CONFLICT,
                    format!("the name `{name}` is already used in this module"),
                )
                .with_primary(span, "imported here")
                .with_secondary(*first, "the name is already used here")
                .with_help("import the module instead (`use a.b`) and write `b.name`"),
            );
        }
        return;
    }
    scope.names.insert(name.to_string(), (binding, span));
}

fn private_item(name: &str, use_span: Span, decl_span: Span) -> Diagnostic {
    Diagnostic::error(
        codes::PRIVATE_ITEM,
        format!("`{name}` is private to its module"),
    )
    .with_primary(use_span, "used from another module")
    .with_secondary(decl_span, "declared here without `pub`")
    .with_help(format!("add `pub` to the declaration of `{name}`"))
}

/// Renames declarations and rewrites references in one module.
struct Rewriter<'a> {
    scope: &'a Scope,
    exports: &'a [Exports],
    /// Unique enum name → its module prefix, for `Shape.Circle`.
    enum_prefixes: &'a HashMap<String, String>,
    /// Local variable names, innermost scope last.
    locals: Vec<Vec<String>>,
    diagnostics: &'a mut Vec<Diagnostic>,
}

impl Rewriter<'_> {
    fn item(&mut self, item: &mut Item, prefix: &str) {
        match item {
            Item::Fn(f) => {
                f.name.name = format!("{prefix}{}", f.name.name);
                for p in &mut f.params {
                    self.ty(&mut p.ty);
                }
                if let Some(ret) = &mut f.ret {
                    self.ty(ret);
                }
                self.locals = vec![f.params.iter().map(|p| p.name.name.clone()).collect()];
                self.block(&mut f.body);
            }
            Item::Struct(s) => {
                s.name.name = format!("{prefix}{}", s.name.name);
                for field in &mut s.fields {
                    self.ty(&mut field.ty);
                    if let Some(default) = &mut field.default {
                        self.locals = vec![Vec::new()];
                        self.expr(default);
                    }
                }
            }
            Item::Enum(e) => {
                e.name.name = format!("{prefix}{}", e.name.name);
                for v in &mut e.variants {
                    v.name.name = format!("{prefix}{}", v.name.name);
                    for field in &mut v.fields {
                        self.ty(&mut field.ty);
                    }
                }
            }
        }
    }

    fn is_local(&self, name: &str) -> bool {
        self.locals
            .iter()
            .any(|scope| scope.iter().any(|n| n == name))
    }

    fn declare(&mut self, name: &str) {
        if let Some(scope) = self.locals.last_mut() {
            scope.push(name.to_string());
        }
    }

    /// `module.name`: the unique name of a public item of `module`.
    fn qualified(&mut self, module: usize, name: &Ident) -> Option<String> {
        match self.exports[module].items.get(&name.name) {
            Some(export) => {
                if !export.is_pub {
                    self.diagnostics
                        .push(private_item(&name.name, name.span, export.span));
                }
                Some(export.full.clone())
            }
            None => {
                self.diagnostics.push(
                    Diagnostic::error(
                        codes::UNKNOWN_IMPORT,
                        format!("the module has no item `{}`", name.name),
                    )
                    .with_primary(name.span, "not declared in that module"),
                );
                None
            }
        }
    }

    fn module_alias(&self, name: &str) -> Option<usize> {
        if self.is_local(name) {
            return None;
        }
        match self.scope.names.get(name) {
            Some((Binding::Module(m), _)) => Some(*m),
            _ => None,
        }
    }

    fn ty(&mut self, t: &mut TypeExpr) {
        match &mut t.kind {
            TypeExprKind::Unit => {}
            TypeExprKind::Optional(inner) => self.ty(inner),
            TypeExprKind::Named { name, args } => {
                for arg in args {
                    self.ty(arg);
                }
                if let Some((module, rest)) = name.name.split_once('.') {
                    if let Some(m) = self.module_alias(module) {
                        let item = Ident {
                            name: rest.to_string(),
                            span: name.span,
                        };
                        if let Some(full) = self.qualified(m, &item) {
                            name.name = full;
                        }
                    }
                    return;
                }
                if let Some((Binding::Item(full), _)) = self.scope.names.get(&name.name) {
                    name.name = full.clone();
                }
            }
        }
    }

    fn block(&mut self, b: &mut Block) {
        self.locals.push(Vec::new());
        for s in &mut b.stmts {
            self.stmt(s);
        }
        self.locals.pop();
    }

    fn stmt(&mut self, s: &mut Stmt) {
        match &mut s.kind {
            StmtKind::Let { name, ty, init, .. } => {
                if let Some(t) = ty {
                    self.ty(t);
                }
                self.expr(init);
                self.declare(&name.name.clone());
            }
            StmtKind::Assign { target, value, .. } => {
                self.expr(target);
                self.expr(value);
            }
            StmtKind::Expr(e) => self.expr(e),
            StmtKind::While { cond, body } => {
                self.expr(cond);
                self.block(body);
            }
            StmtKind::For { var, iter, body } => {
                self.expr(iter);
                self.locals.push(vec![var.name.clone()]);
                self.block(body);
                self.locals.pop();
            }
            StmtKind::Return(Some(e)) => self.expr(e),
            StmtKind::Return(None) | StmtKind::Break | StmtKind::Continue => {}
        }
    }

    /// If `e` names an enum (after rewriting), the prefix its variants use.
    fn enum_prefix(&self, e: &Expr) -> Option<String> {
        match &e.kind {
            ExprKind::Name(n) if !self.is_local(n) => self.enum_prefixes.get(n).cloned(),
            _ => None,
        }
    }

    fn expr(&mut self, e: &mut Expr) {
        // `module.item` and `module.item(args)` become plain references.
        match &mut e.kind {
            ExprKind::Field { expr, field } => {
                if let ExprKind::Name(m) = &expr.kind {
                    if let Some(module) = self.module_alias(m) {
                        if let Some(full) = self.qualified(module, &field.clone()) {
                            e.kind = ExprKind::Name(full);
                        }
                        return;
                    }
                }
            }
            ExprKind::MethodCall {
                receiver,
                method,
                args,
            } => {
                if let ExprKind::Name(m) = &receiver.kind {
                    if let Some(module) = self.module_alias(m) {
                        let full = self
                            .qualified(module, &method.clone())
                            .unwrap_or_else(|| method.name.clone());
                        let callee = Expr::new(ExprKind::Name(full), method.span);
                        let args = std::mem::take(args);
                        e.kind = ExprKind::Call {
                            callee: Box::new(callee),
                            args,
                        };
                        if let ExprKind::Call { args, .. } = &mut e.kind {
                            for arg in args {
                                self.expr(&mut arg.value);
                            }
                        }
                        return;
                    }
                }
            }
            _ => {}
        }

        match &mut e.kind {
            ExprKind::Int(_)
            | ExprKind::Float(_)
            | ExprKind::Bool(_)
            | ExprKind::Char(_)
            | ExprKind::None
            | ExprKind::Unit => {}
            ExprKind::Name(n) => {
                if self.is_local(n) {
                    return;
                }
                match self.scope.names.get(n.as_str()) {
                    Some((Binding::Item(full), _)) => *n = full.clone(),
                    Some((Binding::Module(_), _)) => self.diagnostics.push(
                        Diagnostic::error(
                            codes::MODULE_AS_VALUE,
                            format!("`{n}` is a module, not a value"),
                        )
                        .with_primary(e.span, "module used as a value")
                        .with_help(format!("use one of its items, like `{n}.name`")),
                    ),
                    None => {}
                }
            }
            ExprKind::Str(segments) => {
                for segment in segments {
                    if let StrSegment::Expr(inner) = segment {
                        self.expr(inner);
                    }
                }
            }
            ExprKind::List(items) => {
                for item in items {
                    self.expr(item);
                }
            }
            ExprKind::Unary { expr, .. } | ExprKind::Unwrap(expr) | ExprKind::Take(expr) => {
                self.expr(expr)
            }
            ExprKind::Binary { lhs, rhs, .. } => {
                self.expr(lhs);
                self.expr(rhs);
            }
            ExprKind::Range { start, end, .. } => {
                self.expr(start);
                self.expr(end);
            }
            ExprKind::Call { callee, args } => {
                self.expr(callee);
                for arg in args {
                    self.expr(&mut arg.value);
                }
            }
            ExprKind::MethodCall {
                receiver,
                method,
                args,
            } => {
                self.expr(receiver);
                // `Shape.Circle(...)`: variants are renamed with their
                // enum's module prefix.
                if let Some(prefix) = self.enum_prefix(receiver) {
                    method.name = format!("{prefix}{}", method.name);
                }
                for arg in args {
                    self.expr(&mut arg.value);
                }
            }
            ExprKind::Field { expr, field } => {
                self.expr(expr);
                if let Some(prefix) = self.enum_prefix(expr) {
                    field.name = format!("{prefix}{}", field.name);
                }
            }
            ExprKind::Index { expr, index } => {
                self.expr(expr);
                self.expr(index);
            }
            ExprKind::If { cond, then, else_ } => {
                self.expr(cond);
                self.block(then);
                if let Some(e) = else_ {
                    self.expr(e);
                }
            }
            ExprKind::Match { scrutinee, arms } => {
                self.expr(scrutinee);
                for arm in arms {
                    self.locals.push(Vec::new());
                    self.pattern(&mut arm.pattern);
                    self.block(&mut arm.body);
                    self.locals.pop();
                }
            }
            ExprKind::Block(b) => self.block(b),
        }
    }

    fn pattern(&mut self, p: &mut Pattern) {
        match &mut p.kind {
            PatternKind::Name(name) => {
                if name.starts_with(|c: char| c.is_uppercase()) {
                    if let Some((Binding::Item(full), _)) = self.scope.names.get(name.as_str()) {
                        *name = full.clone();
                    }
                } else {
                    let name = name.clone();
                    self.declare(&name);
                }
            }
            PatternKind::Variant { name, args } => {
                if let Some((Binding::Item(full), _)) = self.scope.names.get(&name.name) {
                    name.name = full.clone();
                }
                for arg in args {
                    self.pattern(arg);
                }
            }
            PatternKind::Wildcard
            | PatternKind::Int(_)
            | PatternKind::Str(_)
            | PatternKind::Bool(_)
            | PatternKind::Char(_) => {}
        }
    }
}
