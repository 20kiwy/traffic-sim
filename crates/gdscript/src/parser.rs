//! Recursive-descent GDScript parser (indentation-delimited blocks).

use crate::ast::*;
use crate::lexer::{lex, Tok, Token};

pub fn parse(src: &str) -> Result<Script, String> {
    let toks = lex(src)?;
    let mut p = Parser { toks, pos: 0 };
    p.parse_program()
}

type PResult<T> = Result<T, String>;

struct Parser {
    toks: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> &Tok {
        &self.toks[self.pos.min(self.toks.len() - 1)].tok
    }

    fn line(&self) -> usize {
        self.toks[self.pos.min(self.toks.len() - 1)].line
    }

    fn next(&mut self) -> Tok {
        let t = self.toks[self.pos.min(self.toks.len() - 1)].tok.clone();
        if self.pos < self.toks.len() {
            self.pos += 1;
        }
        t
    }

    fn eat(&mut self, t: &Tok) -> bool {
        if self.peek() == t {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, t: &Tok, what: &str) -> PResult<()> {
        if self.eat(t) {
            Ok(())
        } else {
            Err(format!(
                "line {}: expected {what}, found {:?}",
                self.line(),
                self.peek()
            ))
        }
    }

    fn skip_newls(&mut self) {
        while self.peek() == &Tok::Newl {
            self.pos += 1;
        }
    }

    fn ident(&mut self) -> PResult<String> {
        match self.next() {
            Tok::Ident(s) => Ok(s),
            other => Err(format!("line {}: expected identifier, found {other:?}", self.line())),
        }
    }

    fn peek_ident(&self) -> Option<String> {
        match self.peek() {
            Tok::Ident(s) => Some(s.clone()),
            _ => None,
        }
    }

    /* ------------------------- program ------------------------- */

    fn parse_program(&mut self) -> PResult<Script> {
        let mut script = Script {
            extends: None,
            init: Vec::new(),
            funcs: Vec::new(),
        };
        loop {
            self.skip_newls();
            match self.peek().clone() {
                Tok::Eof => break,
                Tok::Ident(kw) if kw == "extends" => {
                    self.next();
                    script.extends = Some(self.ident()?);
                }
                Tok::Ident(kw) if kw == "class_name" || kw == "tool" || kw == "@icon" => {
                    self.next();
                    if self.peek_ident().is_some() {
                        let _ = self.ident();
                    }
                }
                Tok::Ident(kw) if kw == "func" => {
                    script.funcs.push(self.parse_func()?);
                }
                Tok::Ident(kw) if kw == "var" || kw == "const" => {
                    script.init.push(self.parse_var_decl()?);
                }
                // top-level statements use the full statement parser so
                // member assignment (p.position = ...) works outside funcs
                // exactly like it does inside them
                _ => {
                    script.init.push(self.parse_stmt()?);
                }
            }
        }
        Ok(script)
    }

    /* ------------------------- statements ------------------------- */

    fn parse_func(&mut self) -> PResult<FuncDecl> {
        let line = self.line();
        self.expect(&Tok::Ident("func".into()), "'func'")?;
        let name = self.ident()?;
        self.expect(&Tok::LParen, "'(' after func name")?;
        let mut params = Vec::new();
        if self.peek() != &Tok::RParen {
            loop {
                params.push(self.ident()?);
                if !self.eat(&Tok::Comma) {
                    break;
                }
                if self.peek() == &Tok::RParen {
                    break;
                }
            }
        }
        self.expect(&Tok::RParen, "')'")?;
        let body = self.parse_block()?;
        Ok(FuncDecl { name, params, body, line })
    }

    fn parse_var_decl(&mut self) -> PResult<Stmt> {
        let line = self.line();
        self.next(); // var / const
        let name = self.ident()?;
        let mut expr = None;
        if self.eat(&Tok::Assign) {
            expr = Some(self.parse_expr()?);
        }
        self.stmt_tail()?;
        Ok(Stmt::VarDecl { name, expr, line })
    }

    /// consume optional trailing newline after a single-line statement
    fn stmt_tail(&mut self) -> PResult<()> {
        match self.peek() {
            Tok::Newl => {
                self.next();
                Ok(())
            }
            Tok::Eof | Tok::Dedent => Ok(()),
            other => Err(format!(
                "line {}: unexpected {:?} after statement (one statement per line)",
                self.line(),
                other
            )),
        }
    }

    fn parse_block(&mut self) -> PResult<Vec<Stmt>> {
        self.expect(&Tok::Colon, "':'")?;
        self.skip_newls();
        if !self.eat(&Tok::Indent) {
            return Err(format!(
                "line {}: expected an indented block",
                self.line()
            ));
        }
        let mut stmts = Vec::new();
        loop {
            self.skip_newls();
            match self.peek() {
                Tok::Dedent => {
                    self.next();
                    break;
                }
                Tok::Eof => break,
                _ => stmts.push(self.parse_stmt()?),
            }
        }
        Ok(stmts)
    }

    fn parse_stmt(&mut self) -> PResult<Stmt> {
        let line = self.line();
        match self.peek().clone() {
            Tok::Ident(kw) => match kw.as_str() {
                "var" | "const" => return self.parse_var_decl(),
                "if" => return self.parse_if(),
                "while" => {
                    self.next();
                    let cond = self.parse_expr()?;
                    let body = self.parse_block()?;
                    return Ok(Stmt::While { cond, body, line });
                }
                "for" => {
                    self.next();
                    let var = self.ident()?;
                    self.expect(&Tok::Ident("in".into()), "'in' in for loop")?;
                    let iter = self.parse_expr()?;
                    let body = self.parse_block()?;
                    return Ok(Stmt::For { var, iter, body, line });
                }
                "return" => {
                    self.next();
                    let expr = match self.peek() {
                        Tok::Newl | Tok::Dedent | Tok::Eof => None,
                        _ => Some(self.parse_expr()?),
                    };
                    self.stmt_tail()?;
                    return Ok(Stmt::Return { expr, line });
                }
                "break" => {
                    self.next();
                    self.stmt_tail()?;
                    return Ok(Stmt::Break { line });
                }
                "continue" => {
                    self.next();
                    self.stmt_tail()?;
                    return Ok(Stmt::Continue { line });
                }
                "pass" => {
                    self.next();
                    self.stmt_tail()?;
                    return Ok(Stmt::Pass);
                }
                "func" => {
                    return Err(format!(
                        "line {line}: nested functions are not supported by the RoadKit script runtime"
                    ));
                }
                "elif" => {
                    return Err(format!("line {line}: 'elif' without matching 'if'"));
                }
                "else" => {
                    return Err(format!("line {line}: 'else' without matching 'if'"));
                }
                _ => {}
            },
            _ => {}
        }

        // expression / assignment statement
        let expr = self.parse_expr()?;
        match self.peek().clone() {
            Tok::Assign => {
                self.next();
                let value = self.parse_expr()?;
                self.stmt_tail()?;
                Self::check_lvalue(&expr, line)?;
                Ok(Stmt::Assign { target: expr, expr: value, line })
            }
            Tok::PlusEq | Tok::MinusEq | Tok::StarEq | Tok::SlashEq => {
                let op = match self.next() {
                    Tok::PlusEq => BinOp::Add,
                    Tok::MinusEq => BinOp::Sub,
                    Tok::StarEq => BinOp::Mul,
                    Tok::SlashEq => BinOp::Div,
                    _ => unreachable!(),
                };
                let value = self.parse_expr()?;
                self.stmt_tail()?;
                Self::check_lvalue(&expr, line)?;
                Ok(Stmt::OpAssign { target: expr, op, expr: value, line })
            }
            _ => {
                self.stmt_tail()?;
                Ok(Stmt::ExprStmt { expr, line })
            }
        }
    }

    fn check_lvalue(e: &Expr, line: usize) -> PResult<()> {
        match e {
            Expr::Ident(_) | Expr::Member(_, _) => Ok(()),
            _ => Err(format!("line {line}: invalid assignment target")),
        }
    }

    fn parse_if(&mut self) -> PResult<Stmt> {
        let line = self.line();
        self.next(); // if
        let cond = self.parse_expr()?;
        let then = self.parse_block()?;

        let mut elifs: Vec<(Expr, Vec<Stmt>)> = Vec::new();
        let mut els: Option<Vec<Stmt>> = None;

        loop {
            let save = self.pos;
            self.skip_newls();
            match self.peek().clone() {
                Tok::Ident(kw) if kw == "elif" => {
                    self.next();
                    let c = self.parse_expr()?;
                    let b = self.parse_block()?;
                    elifs.push((c, b));
                }
                Tok::Ident(kw) if kw == "else" => {
                    self.next();
                    els = Some(self.parse_block()?);
                    break;
                }
                _ => {
                    self.pos = save;
                    break;
                }
            }
        }

        Ok(Stmt::If { cond, then, elifs, els, line })
    }

    /* ------------------------- expressions ------------------------- */

    pub fn parse_expr(&mut self) -> PResult<Expr> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> PResult<Expr> {
        let mut lhs = self.parse_and()?;
        loop {
            if self.peek_ident().as_deref() == Some("or") {
                let line = self.line();
                self.next();
                let rhs = self.parse_and()?;
                lhs = Expr::Binary { op: BinOp::Or, lhs: Box::new(lhs), rhs: Box::new(rhs), line };
            } else {
                break;
            }
        }
        Ok(lhs)
    }

    fn parse_and(&mut self) -> PResult<Expr> {
        let mut lhs = self.parse_not()?;
        loop {
            if self.peek_ident().as_deref() == Some("and") {
                let line = self.line();
                self.next();
                let rhs = self.parse_not()?;
                lhs = Expr::Binary { op: BinOp::And, lhs: Box::new(lhs), rhs: Box::new(rhs), line };
            } else {
                break;
            }
        }
        Ok(lhs)
    }

    fn parse_not(&mut self) -> PResult<Expr> {
        if self.peek_ident().as_deref() == Some("not") {
            self.next();
            let e = self.parse_not()?;
            return Ok(Expr::Unary { op: UnOp::Not, expr: Box::new(e) });
        }
        self.parse_comparison()
    }

    fn parse_comparison(&mut self) -> PResult<Expr> {
        let lhs = self.parse_add()?;
        let op = match self.peek() {
            Tok::Eq => Some(BinOp::Eq),
            Tok::Neq => Some(BinOp::Neq),
            Tok::Lt => Some(BinOp::Lt),
            Tok::Le => Some(BinOp::Le),
            Tok::Gt => Some(BinOp::Gt),
            Tok::Ge => Some(BinOp::Ge),
            _ => None,
        };
        if let Some(op) = op {
            let line = self.line();
            self.next();
            let rhs = self.parse_add()?;
            return Ok(Expr::Binary { op, lhs: Box::new(lhs), rhs: Box::new(rhs), line });
        }
        Ok(lhs)
    }

    fn parse_add(&mut self) -> PResult<Expr> {
        let mut lhs = self.parse_mul()?;
        loop {
            let op = match self.peek() {
                Tok::Plus => BinOp::Add,
                Tok::Minus => BinOp::Sub,
                _ => break,
            };
            let line = self.line();
            self.next();
            let rhs = self.parse_mul()?;
            lhs = Expr::Binary { op, lhs: Box::new(lhs), rhs: Box::new(rhs), line };
        }
        Ok(lhs)
    }

    fn parse_mul(&mut self) -> PResult<Expr> {
        let mut lhs = self.parse_unary()?;
        loop {
            let op = match self.peek() {
                Tok::Star => BinOp::Mul,
                Tok::Slash => BinOp::Div,
                Tok::Percent => BinOp::Mod,
                _ => break,
            };
            let line = self.line();
            self.next();
            let rhs = self.parse_unary()?;
            lhs = Expr::Binary { op, lhs: Box::new(lhs), rhs: Box::new(rhs), line };
        }
        Ok(lhs)
    }

    fn parse_unary(&mut self) -> PResult<Expr> {
        if self.peek() == &Tok::Minus {
            self.next();
            let e = self.parse_unary()?;
            return Ok(Expr::Unary { op: UnOp::Neg, expr: Box::new(e) });
        }
        self.parse_postfix()
    }

    fn parse_postfix(&mut self) -> PResult<Expr> {
        let mut e = self.parse_primary()?;
        loop {
            match self.peek() {
                Tok::Dot => {
                    self.next();
                    let name = self.ident()?;
                    e = Expr::Member(Box::new(e), name);
                }
                Tok::LParen => {
                    let line = self.line();
                    self.next();
                    let mut args = Vec::new();
                    if self.peek() != &Tok::RParen {
                        loop {
                            args.push(self.parse_expr()?);
                            if !self.eat(&Tok::Comma) {
                                break;
                            }
                            if self.peek() == &Tok::RParen {
                                break;
                            }
                        }
                    }
                    self.expect(&Tok::RParen, "')' after call")?;
                    e = Expr::Call { callee: Box::new(e), args, line };
                }
                _ => break,
            }
        }
        Ok(e)
    }

    fn parse_primary(&mut self) -> PResult<Expr> {
        let line = self.line();
        match self.next() {
            Tok::Int(i) => Ok(Expr::Int(i)),
            Tok::Float(f) => Ok(Expr::Float(f)),
            Tok::Str(s) => Ok(Expr::Str(s)),
            Tok::Ident(s) => match s.as_str() {
                "true" => Ok(Expr::Bool(true)),
                "false" => Ok(Expr::Bool(false)),
                "null" | "nil" => Ok(Expr::Nil),
                "self" => Ok(Expr::Ident("self".to_string())),
                _ => Ok(Expr::Ident(s)),
            },
            Tok::LParen => {
                let e = self.parse_expr()?;
                self.expect(&Tok::RParen, "')'")?;
                Ok(e)
            }
            Tok::LBracket => {
                let mut items = Vec::new();
                if self.peek() != &Tok::RBracket {
                    loop {
                        items.push(self.parse_expr()?);
                        if !self.eat(&Tok::Comma) {
                            break;
                        }
                        if self.peek() == &Tok::RBracket {
                            break;
                        }
                    }
                }
                self.expect(&Tok::RBracket, "']'")?;
                Ok(Expr::Array(items))
            }
            other => Err(format!("line {line}: unexpected {other:?} in expression")),
        }
    }
}
