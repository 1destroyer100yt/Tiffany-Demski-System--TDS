//! The TDS parser: tokens → AST.
//!
//! Statements and items are parsed by plain recursive descent. Expressions
//! use a Pratt parser (see [`Parser::expr_bp`]), driven by the binding
//! powers in [`infix_binding_power`].
//!
//! # Error recovery
//!
//! Parse functions return `PResult<T>`. `Err(Reported)` means "an error has
//! already been recorded; give up on this construct". The statement loop
//! then calls [`Parser::sync_line`], which skips the rest of the line and
//! any indented block hanging off it, and parsing carries on with the next
//! statement. Errors at `TokenKind::Error` tokens are never reported, because
//! the lexer already reported them.

use tds_ast::*;
use tds_diagnostics::{codes, Diagnostic, Span};
use tds_lexer::{lex, StrPart, Token, TokenKind};

/// Parses a whole source file. Lexer errors are included in the result.
pub fn parse_module(src: &str) -> (Module, Vec<Diagnostic>) {
    let lexed = lex(src);
    let mut parser = Parser::new(lexed.tokens);
    let mut items = Vec::new();
    for entry in parser.top_level(false) {
        if let TopLevel::Item(item) = entry {
            items.push(item);
        }
    }
    let mut errors = lexed.errors;
    errors.append(&mut parser.errors);
    (Module { items }, errors)
}

/// Parses one REPL entry, where items and statements may be mixed.
pub fn parse_repl(src: &str) -> (Vec<TopLevel>, Vec<Diagnostic>) {
    let lexed = lex(src);
    let mut parser = Parser::new(lexed.tokens);
    let entries = parser.top_level(true);
    let mut errors = lexed.errors;
    errors.append(&mut parser.errors);
    (entries, errors)
}

/// Marker meaning "an error was reported; stop parsing this construct".
#[derive(Debug, Clone, Copy)]
struct Reported;

type PResult<T> = Result<T, Reported>;

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    errors: Vec<Diagnostic>,
    /// Span of the most recently consumed token.
    prev_span: Span,
    /// Whether the most recently consumed token was a DEDENT. A statement
    /// that ends with an indented block has no NEWLINE of its own.
    prev_was_dedent: bool,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Parser {
        debug_assert!(matches!(
            tokens.last(),
            Some(Token {
                kind: TokenKind::Eof,
                ..
            })
        ));
        Parser {
            tokens,
            pos: 0,
            errors: Vec::new(),
            prev_span: Span::empty(0),
            prev_was_dedent: false,
        }
    }

    // ------------------------------------------------------------------
    // Token helpers
    // ------------------------------------------------------------------

    fn peek(&self) -> &TokenKind {
        &self.tokens[self.pos].kind
    }

    fn peek_nth(&self, n: usize) -> &TokenKind {
        let i = (self.pos + n).min(self.tokens.len() - 1);
        &self.tokens[i].kind
    }

    fn peek_span(&self) -> Span {
        self.tokens[self.pos].span
    }

    fn at(&self, kind: &TokenKind) -> bool {
        self.peek() == kind
    }

    fn at_ident(&self) -> bool {
        matches!(self.peek(), TokenKind::Ident(_))
    }

    fn bump(&mut self) -> Token {
        let tok = self.tokens[self.pos].clone();
        if tok.kind != TokenKind::Eof {
            self.pos += 1;
        }
        self.prev_span = tok.span;
        self.prev_was_dedent = tok.kind == TokenKind::Dedent;
        tok
    }

    fn eat(&mut self, kind: &TokenKind) -> bool {
        if self.at(kind) {
            self.bump();
            true
        } else {
            false
        }
    }

    /// Consumes `kind` or reports "expected `what`, found ...".
    fn expect(&mut self, kind: &TokenKind, what: &str) -> PResult<Span> {
        if self.at(kind) {
            return Ok(self.bump().span);
        }
        Err(self.unexpected(what))
    }

    fn expect_ident(&mut self, what: &str) -> PResult<Ident> {
        if let TokenKind::Ident(name) = self.peek() {
            let name = name.clone();
            let span = self.bump().span;
            return Ok(Ident { name, span });
        }
        Err(self.unexpected(what))
    }

    fn error(&mut self, diag: Diagnostic) -> Reported {
        self.errors.push(diag);
        Reported
    }

    /// Reports "expected X, found Y" at the current token, unless the
    /// current token is a lexer error (already reported).
    fn unexpected(&mut self, expected: &str) -> Reported {
        let code = if expected == "an expression" {
            codes::EXPECTED_EXPRESSION
        } else if expected == "a type" {
            codes::EXPECTED_TYPE
        } else {
            codes::UNEXPECTED_TOKEN
        };
        self.unexpected_with(code, expected)
    }

    fn unexpected_with(&mut self, code: &'static str, expected: &str) -> Reported {
        if self.at(&TokenKind::Error) {
            return Reported;
        }
        let found = describe(self.peek());
        let span = self.found_span();
        self.error(
            Diagnostic::error(code, format!("expected {expected}, found {found}"))
                .with_primary(span, format!("expected {expected}")),
        )
    }

    /// Where to point an "expected X" error. Layout tokens at the end of a
    /// line point just past the previous token instead, which is where the
    /// missing thing belongs.
    fn found_span(&self) -> Span {
        match self.peek() {
            TokenKind::Newline | TokenKind::Dedent | TokenKind::Eof => {
                Span::empty(self.prev_span.end)
            }
            _ => self.peek_span(),
        }
    }

    /// Ends a simple statement: needs a NEWLINE, unless the statement ended
    /// with an indented block.
    fn expect_line_end(&mut self) -> PResult<()> {
        if self.prev_was_dedent || self.eat(&TokenKind::Newline) {
            return Ok(());
        }
        if self.at(&TokenKind::Eof) {
            return Ok(());
        }
        Err(self.unexpected("end of line"))
    }

    // ------------------------------------------------------------------
    // Recovery
    // ------------------------------------------------------------------

    /// Skips the rest of the current line and any block hanging off it.
    /// Stops before a DEDENT so the enclosing block can end normally.
    fn sync_line(&mut self) {
        loop {
            match self.peek() {
                TokenKind::Eof | TokenKind::Dedent => return,
                TokenKind::Newline => {
                    self.bump();
                    if self.at(&TokenKind::Indent) {
                        self.skip_block();
                    }
                    return;
                }
                TokenKind::Indent => {
                    self.skip_block();
                    return;
                }
                _ => {
                    self.bump();
                }
            }
        }
    }

    /// Skips from an INDENT to its matching DEDENT.
    fn skip_block(&mut self) {
        let mut depth = 0;
        loop {
            match self.bump().kind {
                TokenKind::Indent => depth += 1,
                TokenKind::Dedent => {
                    depth -= 1;
                    if depth == 0 {
                        return;
                    }
                }
                TokenKind::Eof => return,
                _ => {}
            }
        }
    }

    // ------------------------------------------------------------------
    // Top level and items
    // ------------------------------------------------------------------

    fn top_level(&mut self, allow_stmts: bool) -> Vec<TopLevel> {
        let mut entries = Vec::new();
        loop {
            match self.peek() {
                TokenKind::Eof => break,
                TokenKind::Newline | TokenKind::Dedent => {
                    self.bump();
                    continue;
                }
                TokenKind::Indent => {
                    let span = self.peek_span();
                    self.error(
                        Diagnostic::error(codes::UNEXPECTED_INDENT, "unexpected indentation")
                            .with_primary(span, "this line is indented")
                            .with_note("only lines inside a block (after a `:`) are indented")
                            .with_help("remove the indentation"),
                    );
                    self.skip_block();
                    continue;
                }
                _ => {}
            }

            if self.at_item_start() {
                match self.item() {
                    Ok(item) => entries.push(TopLevel::Item(item)),
                    Err(Reported) => self.sync_line(),
                }
            } else if allow_stmts {
                match self.stmt() {
                    Ok(stmt) => entries.push(TopLevel::Stmt(stmt)),
                    Err(Reported) => self.sync_line(),
                }
            } else {
                let span = self.peek_span();
                if !self.at(&TokenKind::Error) {
                    self.error(
                        Diagnostic::error(
                            codes::STATEMENT_AT_TOP_LEVEL,
                            "statements can't appear at the top level of a file",
                        )
                        .with_primary(span, "expected `fn`, `struct` or `enum`")
                        .with_note("code runs from `fn main()`; the top level only declares things")
                        .with_help("move this line into `fn main():`"),
                    );
                }
                self.sync_line();
            }
        }
        entries
    }

    fn at_item_start(&self) -> bool {
        use TokenKind::*;
        matches!(
            self.peek(),
            Fn | Struct
                | Enum
                | Pub
                | Impl
                | Interface
                | Use
                | Const
                | Static
                | Extern
                | Unsafe
                | At
        )
    }

    fn item(&mut self) -> PResult<Item> {
        // `pub` is accepted and ignored: in Phase 1 everything is one module.
        self.eat(&TokenKind::Pub);
        match self.peek() {
            TokenKind::Fn => self.fn_item().map(Item::Fn),
            TokenKind::Struct => self.struct_item().map(Item::Struct),
            TokenKind::Enum => self.enum_item().map(Item::Enum),
            TokenKind::Impl => Err(self.not_supported("`impl` blocks", "Phase 3")),
            TokenKind::Interface => Err(self.not_supported("interfaces", "Phase 3")),
            TokenKind::Use => Err(self.not_supported("`use` and modules", "Phase 2")),
            TokenKind::Const | TokenKind::Static => {
                Err(self.not_supported("constants and statics", "Phase 2"))
            }
            TokenKind::Extern | TokenKind::Unsafe => {
                Err(self.not_supported("`unsafe` and C interop", "Phase 5"))
            }
            TokenKind::At => Err(self.not_supported("attributes like `@derive`", "Phase 3")),
            _ => Err(self.unexpected_with(codes::EXPECTED_ITEM, "`fn`, `struct` or `enum`")),
        }
    }

    /// Reports a feature that belongs to a later phase.
    fn not_supported(&mut self, what: &str, phase: &str) -> Reported {
        let span = self.peek_span();
        self.error(
            Diagnostic::error(
                codes::NOT_SUPPORTED_YET,
                format!("{what} aren't supported yet"),
            )
            .with_primary(span, "not available in the Phase 1 prototype")
            .with_note(format!("{what} arrive in {phase} of the TDS roadmap")),
        )
    }

    fn fn_item(&mut self) -> PResult<FnDecl> {
        let start = self.bump().span; // `fn`
        let name = self.expect_ident("a function name")?;
        if self.at(&TokenKind::LBracket) {
            return Err(self.not_supported("generic functions", "Phase 3"));
        }
        self.expect(&TokenKind::LParen, "`(`")?;
        let mut params = Vec::new();
        while !self.at(&TokenKind::RParen) {
            params.push(self.param()?);
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }
        self.expect(&TokenKind::RParen, "`,` or `)`")?;

        let ret = if self.eat(&TokenKind::Arrow) {
            Some(self.type_expr()?)
        } else {
            None
        };

        let body = if self.eat(&TokenKind::Eq) {
            // `fn square(x: Int) -> Int = x * x`
            let e = self.expr()?;
            let span = e.span;
            self.expect_line_end()?;
            Block {
                stmts: vec![Stmt {
                    kind: StmtKind::Expr(e),
                    span,
                }],
                span,
            }
        } else if self.at(&TokenKind::Colon) {
            self.bump();
            self.body()?
        } else {
            let err = self.unexpected("`:` or `=`");
            if let Some(diag) = self.errors.last_mut() {
                diag.help
                    .push("start the function body with `:` and an indented block".into());
            }
            return Err(err);
        };

        Ok(FnDecl {
            span: start.to(self.prev_span),
            name,
            params,
            ret,
            body,
        })
    }

    fn param(&mut self) -> PResult<Param> {
        if self.at(&TokenKind::SelfValue) {
            return Err(self.not_supported("methods and `self`", "Phase 3"));
        }
        let name = self.expect_ident("a parameter name")?;
        if !self.at(&TokenKind::Colon) {
            let span = Span::empty(name.span.end);
            return Err(self.error(
                Diagnostic::error(
                    codes::MISSING_PARAM_TYPE,
                    format!("parameter `{}` needs a type", name.name),
                )
                .with_primary(span, "expected `: Type` here")
                .with_note("function signatures are always written out in full")
                .with_help(format!("for example: `{}: Int`", name.name)),
            ));
        }
        self.bump();
        let mode = match self.peek() {
            TokenKind::Mut => {
                self.bump();
                Mode::Mut
            }
            TokenKind::Take => {
                self.bump();
                Mode::Take
            }
            _ => Mode::Read,
        };
        let ty = self.type_expr()?;
        Ok(Param {
            span: name.span.to(ty.span),
            name,
            mode,
            ty,
        })
    }

    fn struct_item(&mut self) -> PResult<StructDecl> {
        let start = self.bump().span; // `struct`
        let name = self.expect_ident("a struct name")?;
        if self.at(&TokenKind::LBracket) {
            return Err(self.not_supported("generic structs", "Phase 3"));
        }
        self.expect(&TokenKind::Colon, "`:`")?;
        let mut fields = Vec::new();
        self.indented(|p| {
            let name = p.expect_ident("a field name")?;
            p.expect(&TokenKind::Colon, "`:` and a type")?;
            let ty = p.type_expr()?;
            let default = if p.eat(&TokenKind::Eq) {
                Some(p.expr()?)
            } else {
                None
            };
            p.expect_line_end()?;
            fields.push(FieldDecl {
                span: name.span.to(p.prev_span),
                name,
                ty,
                default,
            });
            Ok(())
        })?;
        Ok(StructDecl {
            span: start.to(self.prev_span),
            name,
            fields,
        })
    }

    fn enum_item(&mut self) -> PResult<EnumDecl> {
        let start = self.bump().span; // `enum`
        let name = self.expect_ident("an enum name")?;
        if self.at(&TokenKind::LBracket) {
            return Err(self.not_supported("generic enums", "Phase 3"));
        }
        self.expect(&TokenKind::Colon, "`:`")?;
        let mut variants = Vec::new();
        self.indented(|p| {
            let name = p.expect_ident("a variant name")?;
            let mut fields = Vec::new();
            if p.eat(&TokenKind::LParen) {
                while !p.at(&TokenKind::RParen) {
                    let field_name = if p.at_ident() && p.peek_nth(1) == &TokenKind::Colon {
                        let n = p.expect_ident("a field name")?;
                        p.bump(); // `:`
                        Some(n)
                    } else {
                        None
                    };
                    let ty = p.type_expr()?;
                    fields.push(VariantField {
                        name: field_name,
                        ty,
                    });
                    if !p.eat(&TokenKind::Comma) {
                        break;
                    }
                }
                p.expect(&TokenKind::RParen, "`,` or `)`")?;
            }
            p.expect_line_end()?;
            variants.push(VariantDecl {
                span: name.span.to(p.prev_span),
                name,
                fields,
            });
            Ok(())
        })?;
        Ok(EnumDecl {
            span: start.to(self.prev_span),
            name,
            variants,
        })
    }

    /// Parses `NEWLINE INDENT line* DEDENT`, calling `line` for each line
    /// and recovering from errors line by line.
    fn indented(&mut self, mut line: impl FnMut(&mut Parser) -> PResult<()>) -> PResult<()> {
        self.expect_block_start()?;
        while !self.at(&TokenKind::Dedent) && !self.at(&TokenKind::Eof) {
            if line(self).is_err() {
                self.sync_line();
            }
        }
        self.eat(&TokenKind::Dedent);
        Ok(())
    }

    /// Expects the NEWLINE + INDENT that follow a block-opening `:`.
    fn expect_block_start(&mut self) -> PResult<()> {
        if !self.at(&TokenKind::Newline) {
            return Err(self.unexpected("a new line after `:`"));
        }
        let newline = self.bump().span;
        if self.eat(&TokenKind::Indent) {
            return Ok(());
        }
        Err(self.error(
            Diagnostic::error(codes::EXPECTED_BLOCK, "expected an indented block")
                .with_primary(
                    Span::empty(newline.start),
                    "a block should start after this `:`",
                )
                .with_help("indent the lines of the block by 4 more spaces"),
        ))
    }

    // ------------------------------------------------------------------
    // Types
    // ------------------------------------------------------------------

    fn type_expr(&mut self) -> PResult<TypeExpr> {
        let mut ty = match self.peek() {
            TokenKind::LParen if self.peek_nth(1) == &TokenKind::RParen => {
                let start = self.bump().span;
                let end = self.bump().span;
                TypeExpr {
                    kind: TypeExprKind::Unit,
                    span: start.to(end),
                }
            }
            TokenKind::Ident(_) => {
                let name = self.expect_ident("a type")?;
                let mut args = Vec::new();
                if self.eat(&TokenKind::LBracket) {
                    while !self.at(&TokenKind::RBracket) {
                        args.push(self.type_expr()?);
                        if !self.eat(&TokenKind::Comma) {
                            break;
                        }
                    }
                    self.expect(&TokenKind::RBracket, "`,` or `]`")?;
                }
                TypeExpr {
                    span: name.span.to(self.prev_span),
                    kind: TypeExprKind::Named { name, args },
                }
            }
            TokenKind::Ref | TokenKind::Lifetime(_) => {
                return Err(self.not_supported("stored references and lifetimes", "Phase 3"));
            }
            TokenKind::Mut | TokenKind::Take => {
                let span = self.peek_span();
                return Err(self.error(
                    Diagnostic::error(
                        codes::MISPLACED_MODE,
                        "a parameter mode can only be used on parameters",
                    )
                    .with_primary(span, "not allowed here")
                    .with_note("`mut` and `take` describe how a function receives an argument"),
                ));
            }
            _ => return Err(self.unexpected("a type")),
        };
        while self.at(&TokenKind::Question) {
            let end = self.bump().span;
            ty = TypeExpr {
                span: ty.span.to(end),
                kind: TypeExprKind::Optional(Box::new(ty)),
            };
        }
        Ok(ty)
    }

    // ------------------------------------------------------------------
    // Statements
    // ------------------------------------------------------------------

    /// Parses the body after a block-opening `:`: either an indented block
    /// or a single simple statement on the same line.
    fn body(&mut self) -> PResult<Block> {
        if self.at(&TokenKind::Newline) {
            return self.block();
        }
        let stmt = self.simple_stmt()?;
        self.expect_line_end()?;
        Ok(Block {
            span: stmt.span,
            stmts: vec![stmt],
        })
    }

    /// Parses `NEWLINE INDENT stmt* DEDENT`.
    fn block(&mut self) -> PResult<Block> {
        let start = self.peek_span();
        self.expect_block_start()?;
        let mut stmts = Vec::new();
        self.block_contents(&mut stmts);
        self.eat(&TokenKind::Dedent);
        // A block always ends with a DEDENT, so the statement it belongs to
        // needs no NEWLINE of its own.
        self.prev_was_dedent = true;
        Ok(Block {
            span: start.to(self.prev_span),
            stmts,
        })
    }

    fn block_contents(&mut self, stmts: &mut Vec<Stmt>) {
        while !self.at(&TokenKind::Dedent) && !self.at(&TokenKind::Eof) {
            if self.at(&TokenKind::Indent) {
                // An indented line that doesn't follow a `:`. Report it and
                // treat its lines as part of this block.
                let span = self.peek_span();
                self.error(
                    Diagnostic::error(codes::UNEXPECTED_INDENT, "unexpected indentation")
                        .with_primary(span, "this line is indented more than the one before")
                        .with_note("a deeper block can only start after a line ending in `:`")
                        .with_help("line this up with the previous line"),
                );
                self.bump();
                self.block_contents(stmts);
                self.eat(&TokenKind::Dedent);
                continue;
            }
            match self.stmt() {
                Ok(stmt) => stmts.push(stmt),
                Err(Reported) => self.sync_line(),
            }
        }
    }

    fn stmt(&mut self) -> PResult<Stmt> {
        match self.peek() {
            TokenKind::While => self.while_stmt(),
            TokenKind::For => self.for_stmt(),
            TokenKind::Else => {
                let span = self.peek_span();
                Err(self.error(
                    Diagnostic::error(codes::ELSE_WITHOUT_IF, "`else` without a matching `if`")
                        .with_primary(span, "no `if` before this")
                        .with_help("line `else` up with its `if`"),
                ))
            }
            TokenKind::Fn | TokenKind::Struct | TokenKind::Enum => {
                Err(self.not_supported("nested declarations", "a later phase"))
            }
            _ => {
                let stmt = self.simple_stmt()?;
                self.expect_line_end()?;
                Ok(stmt)
            }
        }
    }

    /// A statement that can also appear inline after `:` on the same line:
    /// `let`, `return`, `break`, `continue`, assignment or an expression.
    /// Does not consume the end of the line.
    fn simple_stmt(&mut self) -> PResult<Stmt> {
        let start = self.peek_span();
        let kind = match self.peek() {
            TokenKind::Let | TokenKind::Var => return self.let_stmt(),
            TokenKind::Return => {
                self.bump();
                if self.at_stmt_end() {
                    StmtKind::Return(None)
                } else {
                    StmtKind::Return(Some(self.expr()?))
                }
            }
            TokenKind::Break => {
                self.bump();
                StmtKind::Break
            }
            TokenKind::Continue => {
                self.bump();
                StmtKind::Continue
            }
            TokenKind::While | TokenKind::For => {
                return Err(self.unexpected("a simple statement (put loops on their own line)"));
            }
            _ => {
                let e = self.expr()?;
                match assign_op(self.peek()) {
                    Some(op) => {
                        self.bump();
                        self.check_assign_target(&e)?;
                        let value = self.expr()?;
                        StmtKind::Assign {
                            target: e,
                            op,
                            value,
                        }
                    }
                    None => StmtKind::Expr(e),
                }
            }
        };
        Ok(Stmt {
            kind,
            span: start.to(self.prev_span),
        })
    }

    fn at_stmt_end(&self) -> bool {
        matches!(
            self.peek(),
            TokenKind::Newline | TokenKind::Dedent | TokenKind::Eof | TokenKind::Else
        )
    }

    fn check_assign_target(&mut self, target: &Expr) -> PResult<()> {
        match target.kind {
            ExprKind::Name(_) | ExprKind::Field { .. } | ExprKind::Index { .. } => Ok(()),
            _ => Err(self.error(
                Diagnostic::error(
                    codes::INVALID_ASSIGNMENT_TARGET,
                    "can't assign to this expression",
                )
                .with_primary(target.span, "not a variable, field or list element")
                .with_note("the left side of `=` must be a place that can hold a value"),
            )),
        }
    }

    fn let_stmt(&mut self) -> PResult<Stmt> {
        let start = self.peek_span();
        let mutable = self.bump().kind == TokenKind::Var;
        let name = self.expect_ident("a variable name")?;
        let ty = if self.eat(&TokenKind::Colon) {
            Some(self.type_expr()?)
        } else {
            None
        };
        if !self.at(&TokenKind::Eq) {
            if matches!(
                self.peek(),
                TokenKind::Newline | TokenKind::Eof | TokenKind::Dedent
            ) {
                let span = start.to(self.prev_span);
                let keyword = if mutable { "var" } else { "let" };
                return Err(self.error(
                    Diagnostic::error(
                        codes::MISSING_INITIALIZER,
                        format!("`{keyword} {}` needs an initial value", name.name),
                    )
                    .with_primary(span, "no `=` here")
                    .with_note("every variable must hold a value from the moment it is declared")
                    .with_help(format!(
                        "give it a starting value: `{keyword} {} = ...`",
                        name.name
                    )),
                ));
            }
            return Err(self.unexpected("`=`"));
        }
        self.bump();
        let init = self.expr()?;
        Ok(Stmt {
            kind: StmtKind::Let {
                mutable,
                name,
                ty,
                init,
            },
            span: start.to(self.prev_span),
        })
    }

    fn while_stmt(&mut self) -> PResult<Stmt> {
        let start = self.bump().span;
        let cond = self.expr()?;
        self.expect_block_colon()?;
        let body = self.body()?;
        Ok(Stmt {
            kind: StmtKind::While { cond, body },
            span: start.to(self.prev_span),
        })
    }

    fn for_stmt(&mut self) -> PResult<Stmt> {
        let start = self.bump().span;
        let var = self.expect_ident("a loop variable name")?;
        self.expect(&TokenKind::In, "`in`")?;
        let iter = self.expr()?;
        self.expect_block_colon()?;
        let body = self.body()?;
        Ok(Stmt {
            kind: StmtKind::For { var, iter, body },
            span: start.to(self.prev_span),
        })
    }

    /// Expects the `:` after a condition, with a special message for the
    /// classic `if x = 1:` mistake.
    fn expect_block_colon(&mut self) -> PResult<()> {
        if self.eat(&TokenKind::Colon) {
            return Ok(());
        }
        if self.at(&TokenKind::Eq) {
            let span = self.peek_span();
            return Err(self.error(
                Diagnostic::error(
                    codes::ASSIGN_IN_CONDITION,
                    "`=` assigns; it can't be used in a condition",
                )
                .with_primary(span, "assignment here")
                .with_help("to compare two values, use `==`"),
            ));
        }
        Err(self.unexpected("`:`"))
    }

    // ------------------------------------------------------------------
    // Expressions
    // ------------------------------------------------------------------

    fn expr(&mut self) -> PResult<Expr> {
        self.expr_bp(0)
    }

    /// Pratt parser: parses an expression whose operators all bind at least
    /// as tightly as `min_bp`.
    fn expr_bp(&mut self, min_bp: u8) -> PResult<Expr> {
        let mut lhs = self.prefix()?;
        while let Some((op, left_bp, right_bp)) = infix_binding_power(self.peek()) {
            if left_bp < min_bp {
                break;
            }
            let op_span = self.bump().span;
            let rhs = self.expr_bp(right_bp)?;
            let span = lhs.span.to(rhs.span);
            lhs = match op {
                Infix::Binary(op) => Expr {
                    kind: ExprKind::Binary {
                        op,
                        lhs: Box::new(lhs),
                        rhs: Box::new(rhs),
                    },
                    span,
                },
                Infix::Range { inclusive } => Expr {
                    kind: ExprKind::Range {
                        start: Box::new(lhs),
                        end: Box::new(rhs),
                        inclusive,
                    },
                    span,
                },
            };

            // Comparisons don't chain: `a < b < c` is an error, not
            // `(a < b) < c`.
            if let Some((Infix::Binary(next), _, _)) = infix_binding_power(self.peek()) {
                if matches!(op, Infix::Binary(op) if op.is_comparison()) && next.is_comparison() {
                    let next_span = self.peek_span();
                    return Err(self.error(
                        Diagnostic::error(
                            codes::CHAINED_COMPARISON,
                            "comparisons can't be chained",
                        )
                        .with_primary(next_span, "second comparison")
                        .with_secondary(op_span, "first comparison")
                        .with_help("combine them with `and`, like `a < b and b < c`"),
                    ));
                }
            }
        }
        Ok(lhs)
    }

    /// Prefix operators and everything that binds tighter.
    fn prefix(&mut self) -> PResult<Expr> {
        let start = self.peek_span();
        let (kind, bp) = match self.peek() {
            TokenKind::Minus => (Some(UnaryOp::Neg), PREFIX_BP),
            TokenKind::Not => (Some(UnaryOp::Not), NOT_BP),
            TokenKind::Take => (None, PREFIX_BP),
            _ => return self.postfix(),
        };
        self.bump();
        let operand = self.expr_bp(bp)?;
        let span = start.to(operand.span);
        let kind = match kind {
            Some(op) => ExprKind::Unary {
                op,
                expr: Box::new(operand),
            },
            None => ExprKind::Take(Box::new(operand)),
        };
        Ok(Expr { kind, span })
    }

    fn postfix(&mut self) -> PResult<Expr> {
        let mut e = self.primary()?;
        loop {
            match self.peek() {
                TokenKind::LParen => {
                    let args = self.call_args()?;
                    e = Expr {
                        span: e.span.to(self.prev_span),
                        kind: ExprKind::Call {
                            callee: Box::new(e),
                            args,
                        },
                    };
                }
                TokenKind::LBracket => {
                    self.bump();
                    let index = self.expr()?;
                    self.expect(&TokenKind::RBracket, "`]`")?;
                    e = Expr {
                        span: e.span.to(self.prev_span),
                        kind: ExprKind::Index {
                            expr: Box::new(e),
                            index: Box::new(index),
                        },
                    };
                }
                TokenKind::Dot => {
                    self.bump();
                    let name = self.expect_ident("a field or method name")?;
                    if self.at(&TokenKind::LParen) {
                        let args = self.call_args()?;
                        e = Expr {
                            span: e.span.to(self.prev_span),
                            kind: ExprKind::MethodCall {
                                receiver: Box::new(e),
                                method: name,
                                args,
                            },
                        };
                    } else {
                        e = Expr {
                            span: e.span.to(name.span),
                            kind: ExprKind::Field {
                                expr: Box::new(e),
                                field: name,
                            },
                        };
                    }
                }
                TokenKind::Bang => {
                    let end = self.bump().span;
                    e = Expr {
                        span: e.span.to(end),
                        kind: ExprKind::Unwrap(Box::new(e)),
                    };
                }
                TokenKind::Question => {
                    return Err(self.not_supported("the `?` operator and `Result`", "Phase 3"));
                }
                _ => return Ok(e),
            }
        }
    }

    fn call_args(&mut self) -> PResult<Vec<Arg>> {
        self.bump(); // `(`
        let mut args: Vec<Arg> = Vec::new();
        while !self.at(&TokenKind::RParen) {
            let start = self.peek_span();
            let label = if self.at_ident() && self.peek_nth(1) == &TokenKind::Colon {
                let label = self.expect_ident("a name")?;
                self.bump(); // `:`
                Some(label)
            } else {
                None
            };
            let (mode, mode_span) = match self.peek() {
                TokenKind::Mut => (Mode::Mut, Some(self.bump().span)),
                TokenKind::Take => (Mode::Take, Some(self.bump().span)),
                _ => (Mode::Read, None),
            };
            let value = self.expr()?;
            let span = start.to(value.span);
            if label.is_none() {
                if let Some(named) = args.iter().find(|a| a.label.is_some()) {
                    let named_span = named.span;
                    return Err(self.error(
                        Diagnostic::error(
                            codes::POSITIONAL_AFTER_NAMED,
                            "positional argument after a named argument",
                        )
                        .with_primary(span, "positional argument")
                        .with_secondary(named_span, "named argument")
                        .with_help("put positional arguments first, or name this one too"),
                    ));
                }
            }
            args.push(Arg {
                label,
                mode,
                mode_span,
                value,
                span,
            });
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }
        self.expect(&TokenKind::RParen, "`,` or `)`")?;
        Ok(args)
    }

    fn primary(&mut self) -> PResult<Expr> {
        let span = self.peek_span();
        let kind = match self.peek().clone() {
            TokenKind::Int(n) => {
                self.bump();
                ExprKind::Int(n)
            }
            TokenKind::Float(x) => {
                self.bump();
                ExprKind::Float(x)
            }
            TokenKind::Char(c) => {
                self.bump();
                ExprKind::Char(c)
            }
            TokenKind::True => {
                self.bump();
                ExprKind::Bool(true)
            }
            TokenKind::False => {
                self.bump();
                ExprKind::Bool(false)
            }
            TokenKind::None => {
                self.bump();
                ExprKind::None
            }
            TokenKind::Ident(name) => {
                self.bump();
                ExprKind::Name(name)
            }
            TokenKind::Str(parts) => {
                self.bump();
                ExprKind::Str(self.string_segments(parts)?)
            }
            TokenKind::LParen => {
                self.bump();
                if self.eat(&TokenKind::RParen) {
                    ExprKind::Unit
                } else {
                    let inner = self.expr()?;
                    if self.at(&TokenKind::Comma) {
                        return Err(self.not_supported("tuples", "a later phase"));
                    }
                    self.expect(&TokenKind::RParen, "`)`")?;
                    // Keep the parenthesized span so errors cover the parens.
                    return Ok(Expr {
                        kind: inner.kind,
                        span: span.to(self.prev_span),
                    });
                }
            }
            TokenKind::LBracket => {
                self.bump();
                let mut items = Vec::new();
                while !self.at(&TokenKind::RBracket) {
                    items.push(self.expr()?);
                    if !self.eat(&TokenKind::Comma) {
                        break;
                    }
                }
                self.expect(&TokenKind::RBracket, "`,` or `]`")?;
                ExprKind::List(items)
            }
            TokenKind::If => return self.if_expr(),
            TokenKind::Match => return self.match_expr(),
            TokenKind::Fn => return Err(self.not_supported("closures", "Phase 4")),
            TokenKind::SelfValue => return Err(self.not_supported("methods and `self`", "Phase 3")),
            TokenKind::Mut => {
                return Err(self.error(
                    Diagnostic::error(codes::MISPLACED_MODE, "`mut` can only mark a call argument")
                        .with_primary(span, "not allowed here")
                        .with_note(
                            "write `mut` in a call to lend a variable for changing: `f(mut x)`",
                        )
                        .with_help("to declare a variable that can change, use `var`"),
                ));
            }
            _ => return Err(self.unexpected("an expression")),
        };
        Ok(Expr {
            kind,
            span: span.to(self.prev_span),
        })
    }

    /// Converts lexed string parts into AST segments, parsing each
    /// interpolated expression with a sub-parser.
    fn string_segments(&mut self, parts: Vec<StrPart>) -> PResult<Vec<StrSegment>> {
        let mut segments = Vec::new();
        for part in parts {
            match part {
                StrPart::Text(text) => segments.push(StrSegment::Text(text)),
                StrPart::Interp { mut tokens, span } => {
                    tokens.push(Token {
                        kind: TokenKind::Eof,
                        span: Span::empty(span.end),
                    });
                    let mut sub = Parser::new(tokens);
                    let result = sub.expr();
                    if result.is_ok() && !sub.at(&TokenKind::Eof) {
                        sub.unexpected("`}` to end the interpolation");
                    }
                    self.errors.append(&mut sub.errors);
                    segments.push(StrSegment::Expr(result?));
                }
            }
        }
        Ok(segments)
    }

    fn if_expr(&mut self) -> PResult<Expr> {
        let start = self.bump().span; // `if`
        let cond = self.expr()?;
        self.expect_block_colon()?;
        let (then, inline) = self.branch()?;

        // An inline `if a: x` may have its `else` on the next line.
        if inline && self.at(&TokenKind::Newline) && self.peek_nth(1) == &TokenKind::Else {
            self.bump();
        }

        let else_ = if self.at(&TokenKind::Else) {
            self.bump();
            if self.at(&TokenKind::If) {
                Some(Box::new(self.if_expr()?))
            } else {
                let else_start = self.peek_span();
                self.expect(&TokenKind::Colon, "`:` or `if`")?;
                let (block, _) = self.branch()?;
                Some(Box::new(Expr {
                    span: else_start.to(self.prev_span),
                    kind: ExprKind::Block(block),
                }))
            }
        } else {
            None
        };

        Ok(Expr {
            span: start.to(self.prev_span),
            kind: ExprKind::If {
                cond: Box::new(cond),
                then,
                else_,
            },
        })
    }

    /// The body of an `if` / `else` / match arm, after the `:`. Returns the
    /// block and whether it was written inline on the same line.
    fn branch(&mut self) -> PResult<(Block, bool)> {
        if self.at(&TokenKind::Newline) {
            return Ok((self.block()?, false));
        }
        let stmt = self.simple_stmt()?;
        Ok((
            Block {
                span: stmt.span,
                stmts: vec![stmt],
            },
            true,
        ))
    }

    fn match_expr(&mut self) -> PResult<Expr> {
        let start = self.bump().span; // `match`
        let scrutinee = self.expr()?;
        self.expect(&TokenKind::Colon, "`:`")?;
        let mut arms = Vec::new();
        self.indented(|p| {
            let pattern = p.pattern()?;
            p.expect(&TokenKind::Colon, "`:` after the pattern")?;
            let (body, _) = p.branch()?;
            p.expect_line_end()?;
            arms.push(MatchArm {
                span: pattern.span.to(p.prev_span),
                pattern,
                body,
            });
            Ok(())
        })?;
        self.prev_was_dedent = true;
        Ok(Expr {
            span: start.to(self.prev_span),
            kind: ExprKind::Match {
                scrutinee: Box::new(scrutinee),
                arms,
            },
        })
    }

    fn pattern(&mut self) -> PResult<Pattern> {
        let start = self.peek_span();
        let kind = match self.peek().clone() {
            TokenKind::Underscore => {
                self.bump();
                PatternKind::Wildcard
            }
            TokenKind::Ident(_) => {
                let name = self.expect_ident("a pattern")?;
                if self.eat(&TokenKind::LParen) {
                    let mut args = Vec::new();
                    while !self.at(&TokenKind::RParen) {
                        args.push(self.pattern()?);
                        if !self.eat(&TokenKind::Comma) {
                            break;
                        }
                    }
                    self.expect(&TokenKind::RParen, "`,` or `)`")?;
                    PatternKind::Variant { name, args }
                } else {
                    PatternKind::Name(name.name)
                }
            }
            TokenKind::Int(_) | TokenKind::Minus => {
                let negative = self.eat(&TokenKind::Minus);
                let TokenKind::Int(n) = self.peek().clone() else {
                    return Err(self.unexpected_with(codes::EXPECTED_PATTERN, "a number"));
                };
                self.bump();
                let value = if negative {
                    0i64.checked_sub_unsigned(n)
                } else {
                    i64::try_from(n).ok()
                };
                match value {
                    Some(v) => PatternKind::Int(v),
                    None => {
                        let span = start.to(self.prev_span);
                        return Err(self.error(
                            Diagnostic::error(
                                codes::INTEGER_TOO_LARGE,
                                "integer pattern is out of range",
                            )
                            .with_primary(span, "doesn't fit in `Int`"),
                        ));
                    }
                }
            }
            TokenKind::Str(parts) => {
                self.bump();
                match parts.as_slice() {
                    [] => PatternKind::Str(String::new()),
                    [StrPart::Text(text)] => PatternKind::Str(text.clone()),
                    _ => {
                        let span = start.to(self.prev_span);
                        return Err(self.error(
                            Diagnostic::error(
                                codes::EXPECTED_PATTERN,
                                "string patterns can't use interpolation",
                            )
                            .with_primary(span, "interpolation in a pattern")
                            .with_help("compare with `==` in an `if` instead"),
                        ));
                    }
                }
            }
            TokenKind::True => {
                self.bump();
                PatternKind::Bool(true)
            }
            TokenKind::False => {
                self.bump();
                PatternKind::Bool(false)
            }
            TokenKind::Char(c) => {
                self.bump();
                PatternKind::Char(c)
            }
            TokenKind::None => {
                return Err(self.not_supported("patterns on optionals", "a later phase"))
            }
            _ => return Err(self.unexpected_with(codes::EXPECTED_PATTERN, "a pattern")),
        };
        Ok(Pattern {
            kind,
            span: start.to(self.prev_span),
        })
    }
}

// ======================================================================
// Operator tables
// ======================================================================

/// Binding power of `-x` and `take x`: tighter than any binary operator.
const PREFIX_BP: u8 = 17;
/// Binding power of `not x`: looser than comparisons, so `not a == b`
/// means `not (a == b)`, as in Python.
const NOT_BP: u8 = 5;

#[derive(Debug, Clone, Copy)]
enum Infix {
    Binary(BinaryOp),
    Range { inclusive: bool },
}

/// `(operator, left binding power, right binding power)` for an infix
/// token. Higher binds tighter. Left-associative operators have
/// `right = left + 1`; the right-associative `??` has `right < left`.
///
/// | Operators              | Power | Associativity |
/// |------------------------|-------|---------------|
/// | `or`                   | 1     | left          |
/// | `and`                  | 3     | left          |
/// | `not` (prefix)         | 5     |               |
/// | `== != < <= > >=`      | 7     | none          |
/// | `??`                   | 10    | right         |
/// | `..<` `...`            | 11    | none          |
/// | `+ -`                  | 13    | left          |
/// | `* / %`                | 15    | left          |
/// | `-` `take` (prefix)    | 17    |               |
fn infix_binding_power(kind: &TokenKind) -> Option<(Infix, u8, u8)> {
    use BinaryOp::*;
    let binary = |op, l, r| Some((Infix::Binary(op), l, r));
    match kind {
        TokenKind::Or => binary(Or, 1, 2),
        TokenKind::And => binary(And, 3, 4),
        TokenKind::EqEq => binary(Eq, 7, 8),
        TokenKind::BangEq => binary(Ne, 7, 8),
        TokenKind::Lt => binary(Lt, 7, 8),
        TokenKind::LtEq => binary(Le, 7, 8),
        TokenKind::Gt => binary(Gt, 7, 8),
        TokenKind::GtEq => binary(Ge, 7, 8),
        TokenKind::QuestionQuestion => binary(Coalesce, 10, 9),
        TokenKind::DotDotLess => Some((Infix::Range { inclusive: false }, 11, 12)),
        TokenKind::DotDotDot => Some((Infix::Range { inclusive: true }, 11, 12)),
        TokenKind::Plus => binary(Add, 13, 14),
        TokenKind::Minus => binary(Sub, 13, 14),
        TokenKind::Star => binary(Mul, 15, 16),
        TokenKind::Slash => binary(Div, 15, 16),
        TokenKind::Percent => binary(Rem, 15, 16),
        _ => None,
    }
}

fn assign_op(kind: &TokenKind) -> Option<AssignOp> {
    Some(match kind {
        TokenKind::Eq => AssignOp::Assign,
        TokenKind::PlusEq => AssignOp::Add,
        TokenKind::MinusEq => AssignOp::Sub,
        TokenKind::StarEq => AssignOp::Mul,
        TokenKind::SlashEq => AssignOp::Div,
        TokenKind::PercentEq => AssignOp::Rem,
        _ => return None,
    })
}

/// A short description of a token for "found X" messages.
fn describe(kind: &TokenKind) -> String {
    match kind {
        TokenKind::Ident(name) => format!("`{name}`"),
        TokenKind::Int(_) | TokenKind::Float(_) => "a number".to_string(),
        TokenKind::Str(_) => "a string".to_string(),
        TokenKind::Char(_) => "a character".to_string(),
        TokenKind::Lifetime(name) => format!("lifetime `'{name}`"),
        TokenKind::Newline => "end of line".to_string(),
        TokenKind::Indent => "an indented block".to_string(),
        TokenKind::Dedent => "end of block".to_string(),
        TokenKind::Eof => "end of file".to_string(),
        other => format!("`{other}`"),
    }
}
