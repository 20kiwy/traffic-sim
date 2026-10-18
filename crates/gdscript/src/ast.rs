//! GDScript AST.

#[derive(Clone, Debug)]
pub struct Script {
    pub extends: Option<String>,
    /// top-level `var`/`const` initializers (run before entry)
    pub init: Vec<Stmt>,
    pub funcs: Vec<FuncDecl>,
}

#[derive(Clone, Debug)]
pub struct FuncDecl {
    pub name: String,
    pub params: Vec<String>,
    pub body: Vec<Stmt>,
    pub line: usize,
}

#[derive(Clone, Debug)]
pub enum Stmt {
    VarDecl {
        name: String,
        expr: Option<Expr>,
        line: usize,
    },
    Assign {
        target: Expr,
        expr: Expr,
        line: usize,
    },
    OpAssign {
        target: Expr,
        op: BinOp,
        expr: Expr,
        line: usize,
    },
    ExprStmt {
        expr: Expr,
        line: usize,
    },
    If {
        cond: Expr,
        then: Vec<Stmt>,
        elifs: Vec<(Expr, Vec<Stmt>)>,
        els: Option<Vec<Stmt>>,
        line: usize,
    },
    While {
        cond: Expr,
        body: Vec<Stmt>,
        line: usize,
    },
    For {
        var: String,
        iter: Expr,
        body: Vec<Stmt>,
        line: usize,
    },
    Return {
        expr: Option<Expr>,
        line: usize,
    },
    Break {
        line: usize,
    },
    Continue {
        line: usize,
    },
    Pass,
}

#[derive(Clone, Debug)]
pub enum Expr {
    Nil,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Ident(String),
    Member(Box<Expr>, String),
    Call {
        callee: Box<Expr>,
        args: Vec<Expr>,
        line: usize,
    },
    Array(Vec<Expr>),
    Unary {
        op: UnOp,
        expr: Box<Expr>,
    },
    Binary {
        op: BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
        line: usize,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    Neq,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum UnOp {
    Neg,
    Not,
}
