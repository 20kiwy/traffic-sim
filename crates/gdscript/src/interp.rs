//! GDScript interpreter — tree-walking, single-threaded, with a host
//! constructor registry (`RoadPoint.new()` etc.) and a step budget to
//! guarantee the editor never hangs on a runaway script.

use crate::ast::*;
use crate::value::{values_equal, Value, Vec3Val};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

pub type CtorFn = Box<dyn FnMut(&mut Interpreter, &[Value]) -> Result<Value, String>>;

const MAX_STEPS: u64 = 8_000_000;
const MAX_DEPTH: usize = 64;
const MAX_OUTPUT: usize = 600;

pub struct Interpreter {
    pub globals: HashMap<String, Value>,
    /// constructors / global host functions registered by the embedder
    pub functions: HashMap<String, CtorFn>,
    output: Vec<String>,
    steps: u64,
    depth: usize,
    rng: u64,
}

enum Flow {
    Normal,
    Return(Value),
    Break,
    Continue,
}

impl Default for Interpreter {
    fn default() -> Self {
        Self::new()
    }
}

impl Interpreter {
    pub fn new() -> Self {
        let seed = web_time::SystemTime::now()
            .duration_since(web_time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos() as u64 ^ d.as_secs())
            .unwrap_or(0x9E3779B97F4A7C15)
            | 1;
        Self {
            globals: HashMap::new(),
            functions: HashMap::new(),
            output: Vec::new(),
            steps: 0,
            depth: 0,
            rng: seed,
        }
    }

    pub fn register_fn(&mut self, name: &str, f: CtorFn) {
        self.functions.insert(name.to_string(), f);
    }

    pub fn set_global(&mut self, name: &str, v: Value) {
        self.globals.insert(name.to_string(), v);
    }

    pub fn print_line(&mut self, s: String) {
        if self.output.len() >= MAX_OUTPUT {
            self.output.remove(0);
        }
        self.output.push(s);
    }

    pub fn take_output(&mut self) -> Vec<String> {
        std::mem::take(&mut self.output)
    }

    fn rand01(&mut self) -> f64 {
        // xorshift64*
        let mut x = self.rng;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.rng = x;
        let v = x.wrapping_mul(0x2545F4914F6CDD1D);
        ((v >> 11) as f64) / ((1u64 << 53) as f64)
    }

    fn step(&mut self) -> Result<(), String> {
        self.steps += 1;
        if self.steps > MAX_STEPS {
            Err("script exceeded the step budget (8M ops) — possible infinite loop".to_string())
        } else {
            Ok(())
        }
    }

    /* ------------------------- run ------------------------- */

    pub fn run(&mut self, script: &Script, entry: &str) -> Result<Value, String> {
        let mut funcs: HashMap<String, FuncDecl> = HashMap::new();
        for f in &script.funcs {
            funcs.insert(f.name.clone(), f.clone());
        }
        let mut scopes = vec![std::mem::take(&mut self.globals)];
        let result = (|| -> Result<Value, String> {
            for st in &script.init {
                self.exec_stmt(st, &mut scopes, &funcs)?;
            }
            let entry_name = if funcs.contains_key(entry) {
                entry.to_string()
            } else if funcs.contains_key("_ready") {
                "_ready".to_string()
            } else if funcs.contains_key("_init") {
                "_init".to_string()
            } else if !script.init.is_empty() {
                // tool-style script: all logic is top-level (Godot runs
                // @tool scripts the same way) — init statements already ran
                return Ok(Value::Nil);
            } else {
                return Err(format!(
                    "no entry function: expected '{entry}' or '_ready' in script"
                ));
            };
            let f = funcs.get(&entry_name).cloned().unwrap();
            self.call_func(&f, &[], &mut scopes, &funcs)
        })();
        self.globals = std::mem::take(&mut scopes[0]);
        result
    }

    fn call_func(
        &mut self,
        f: &FuncDecl,
        args: &[Value],
        scopes: &mut Vec<HashMap<String, Value>>,
        funcs: &HashMap<String, FuncDecl>,
    ) -> Result<Value, String> {
        if self.depth >= MAX_DEPTH {
            return Err(format!(
                "line {}: recursion too deep (>{MAX_DEPTH} frames)",
                f.line
            ));
        }
        self.depth += 1;
        let mut scope = HashMap::new();
        for (i, p) in f.params.iter().enumerate() {
            scope.insert(p.clone(), args.get(i).cloned().unwrap_or(Value::Nil));
        }
        scopes.push(scope);
        let flow = self.exec_block(&f.body, scopes, funcs);
        scopes.pop();
        self.depth -= 1;
        match flow? {
            Flow::Return(v) => Ok(v),
            _ => Ok(Value::Nil),
        }
    }

    /* ------------------------- statements ------------------------- */

    fn exec_block(
        &mut self,
        stmts: &[Stmt],
        scopes: &mut Vec<HashMap<String, Value>>,
        funcs: &HashMap<String, FuncDecl>,
    ) -> Result<Flow, String> {
        for st in stmts {
            match self.exec_stmt(st, scopes, funcs)? {
                Flow::Normal => {}
                other => return Ok(other),
            }
        }
        Ok(Flow::Normal)
    }

    fn exec_stmt(
        &mut self,
        st: &Stmt,
        scopes: &mut Vec<HashMap<String, Value>>,
        funcs: &HashMap<String, FuncDecl>,
    ) -> Result<Flow, String> {
        self.step().map_err(|e| format!("{e}"))?;
        match st {
            Stmt::VarDecl { name, expr, .. } => {
                let v = match expr {
                    Some(e) => self.eval(e, scopes, funcs)?,
                    None => Value::Nil,
                };
                scopes.last_mut().unwrap().insert(name.clone(), v);
                Ok(Flow::Normal)
            }
            Stmt::Assign { target, expr, line } => {
                let v = self.eval(expr, scopes, funcs)?;
                self.assign_lvalue(target, v, scopes, funcs)
                    .map_err(|e| format!("line {line}: {e}"))?;
                Ok(Flow::Normal)
            }
            Stmt::OpAssign { target, op, expr, line } => {
                let cur = self.eval(target, scopes, funcs)?;
                let rhs = self.eval(expr, scopes, funcs)?;
                let v = self.apply_op(*op, cur, rhs, *line)?;
                self.assign_lvalue(target, v, scopes, funcs)
                    .map_err(|e| format!("line {line}: {e}"))?;
                Ok(Flow::Normal)
            }
            Stmt::ExprStmt { expr, .. } => {
                self.eval(expr, scopes, funcs)?;
                Ok(Flow::Normal)
            }
            Stmt::If { cond, then, elifs, els, .. } => {
                let c = self.eval(cond, scopes, funcs)?;
                if c.truthy() {
                    return self.exec_block(then, scopes, funcs);
                }
                for (ec, eb) in elifs {
                    let cv = self.eval(ec, scopes, funcs)?;
                    if cv.truthy() {
                        return self.exec_block(eb, scopes, funcs);
                    }
                }
                if let Some(eb) = els {
                    return self.exec_block(eb, scopes, funcs);
                }
                Ok(Flow::Normal)
            }
            Stmt::While { cond, body, line } => loop {
                self.step().map_err(|e| format!("line {line}: {e}"))?;
                let c = self.eval(cond, scopes, funcs)?;
                if !c.truthy() {
                    return Ok(Flow::Normal);
                }
                match self.exec_block(body, scopes, funcs)? {
                    Flow::Return(v) => return Ok(Flow::Return(v)),
                    Flow::Break => return Ok(Flow::Normal),
                    _ => {}
                }
            },
            Stmt::For { var, iter, body, line } => {
                let it = self.eval(iter, scopes, funcs)?;
                let items: Vec<Value> = match it {
                    Value::Array(a) => a.borrow().clone(),
                    Value::Int(n) => (0..n).map(Value::Int).collect(),
                    other => {
                        return Err(format!(
                            "line {line}: cannot iterate over {other:?}"
                        ))
                    }
                };
                for item in items {
                    self.step().map_err(|e| format!("line {line}: {e}"))?;
                    let mut frame = HashMap::new();
                    frame.insert(var.clone(), item);
                    scopes.push(frame);
                    let flow = self.exec_block(body, scopes, funcs);
                    scopes.pop();
                    match flow? {
                        Flow::Return(v) => return Ok(Flow::Return(v)),
                        Flow::Break => break,
                        _ => {}
                    }
                }
                Ok(Flow::Normal)
            }
            Stmt::Return { expr, .. } => {
                let v = match expr {
                    Some(e) => self.eval(e, scopes, funcs)?,
                    None => Value::Nil,
                };
                Ok(Flow::Return(v))
            }
            Stmt::Break { .. } => Ok(Flow::Break),
            Stmt::Continue { .. } => Ok(Flow::Continue),
            Stmt::Pass => Ok(Flow::Normal),
        }
    }

    fn assign_lvalue(
        &mut self,
        target: &Expr,
        v: Value,
        scopes: &mut Vec<HashMap<String, Value>>,
        funcs: &HashMap<String, FuncDecl>,
    ) -> Result<(), String> {
        match target {
            Expr::Ident(name) => {
                // assign where declared (innermost first), else local
                for scope in scopes.iter_mut().rev() {
                    if scope.contains_key(name.as_str()) {
                        scope.insert(name.clone(), v);
                        return Ok(());
                    }
                }
                scopes.last_mut().unwrap().insert(name.clone(), v);
                Ok(())
            }
            Expr::Member(base, name) => {
                let obj = self.eval(base, scopes, funcs)?;
                match obj {
                    Value::Obj(o) => o
                        .borrow_mut()
                        .set(name, v)
                        .map_err(|e| format!("{e} (setting '{name}')")),
                    other => Err(format!(
                        "cannot assign property '{name}' on {}",
                        other.type_name()
                    )),
                }
            }
            _ => Err("invalid assignment target".to_string()),
        }
    }

    /* ------------------------- expressions ------------------------- */

    fn eval(
        &mut self,
        e: &Expr,
        scopes: &mut Vec<HashMap<String, Value>>,
        funcs: &HashMap<String, FuncDecl>,
    ) -> Result<Value, String> {
        self.step()?;
        match e {
            Expr::Nil => Ok(Value::Nil),
            Expr::Bool(b) => Ok(Value::Bool(*b)),
            Expr::Int(i) => Ok(Value::Int(*i)),
            Expr::Float(f) => Ok(Value::Float(*f)),
            Expr::Str(s) => Ok(Value::str(s.clone())),
            Expr::Array(items) => {
                let mut out = Vec::with_capacity(items.len());
                for it in items {
                    out.push(self.eval(it, scopes, funcs)?);
                }
                Ok(Value::Array(Rc::new(RefCell::new(out))))
            }
            Expr::Ident(name) => {
                if name == "self" {
                    return Ok(Value::Nil);
                }
                for scope in scopes.iter().rev() {
                    if let Some(v) = scope.get(name.as_str()) {
                        return Ok(v.clone());
                    }
                }
                if self.functions.contains_key(name.as_str()) {
                    return Ok(Value::Class(name.clone()));
                }
                Err(format!("unknown identifier '{name}'"))
            }
            Expr::Member(base, name) => {
                let b = self.eval(base, scopes, funcs)?;
                match b {
                    Value::Obj(o) => o
                        .borrow()
                        .get(name)
                        .ok_or_else(|| {
                            format!(
                                "{} has no property '{name}'",
                                o.borrow().class_name()
                            )
                        }),
                    Value::Vec3(v) => match name.as_str() {
                        "x" => Ok(Value::Float(v.x as f64)),
                        "y" => Ok(Value::Float(v.y as f64)),
                        "z" => Ok(Value::Float(v.z as f64)),
                        _ => Err(format!("Vector3 has no property '{name}'")),
                    },
                    Value::Str(s) => match name.as_str() {
                        "length" => Ok(Value::Int(s.chars().count() as i64)),
                        _ => Err(format!("String has no property '{name}'")),
                    },
                    other => Err(format!(
                        "cannot read property '{name}' on {}",
                        other.type_name()
                    )),
                }
            }
            Expr::Call { callee, args, line } => self.eval_call(callee, args, *line, scopes, funcs),
            Expr::Unary { op, expr } => {
                let v = self.eval(expr, scopes, funcs)?;
                match op {
                    UnOp::Neg => match v {
                        Value::Int(i) => Ok(Value::Int(-i)),
                        Value::Float(f) => Ok(Value::Float(-f)),
                        Value::Vec3(v3) => Ok(Value::Vec3(Vec3Val::new(-v3.x, -v3.y, -v3.z))),
                        other => Err(format!("cannot negate {other:?}")),
                    },
                    UnOp::Not => Ok(Value::Bool(!v.truthy())),
                }
            }
            Expr::Binary { op, lhs, rhs, line } => {
                // short-circuit and/or
                if matches!(op, BinOp::And | BinOp::Or) {
                    let l = self.eval(lhs, scopes, funcs)?;
                    if *op == BinOp::And {
                        if !l.truthy() {
                            return Ok(Value::Bool(false));
                        }
                        let r = self.eval(rhs, scopes, funcs)?;
                        return Ok(Value::Bool(r.truthy()));
                    } else {
                        if l.truthy() {
                            return Ok(Value::Bool(true));
                        }
                        let r = self.eval(rhs, scopes, funcs)?;
                        return Ok(Value::Bool(r.truthy()));
                    }
                }
                let l = self.eval(lhs, scopes, funcs)?;
                let r = self.eval(rhs, scopes, funcs)?;
                self.apply_op(*op, l, r, *line)
            }
        }
    }

    fn eval_call(
        &mut self,
        callee: &Expr,
        args: &[Expr],
        line: usize,
        scopes: &mut Vec<HashMap<String, Value>>,
        funcs: &HashMap<String, FuncDecl>,
    ) -> Result<Value, String> {
        // evaluate arguments first
        let mut vals: Vec<Value> = Vec::with_capacity(args.len());
        for a in args {
            vals.push(self.eval(a, scopes, funcs)?);
        }
        let err = |m: String| format!("line {line}: {m}");

        match callee {
            Expr::Ident(name) => {
                if let Some(f) = funcs.get(name.as_str()).cloned() {
                    return self
                        .call_func(&f, &vals, scopes, funcs)
                        .map_err(err);
                }
                if let Some(mut f) = self.functions.remove(name.as_str()) {
                    let r = f(self, &vals);
                    self.functions.insert(name.clone(), f);
                    return r.map_err(err);
                }
                match name.as_str() {
                    "print" => {
                        let s = vals
                            .iter()
                            .map(Value::to_display)
                            .collect::<Vec<_>>()
                            .join(" ");
                        self.print_line(s);
                        Ok(Value::Nil)
                    }
                    "str" => Ok(Value::str(
                        vals.iter().map(Value::to_display).collect::<Vec<_>>().join(""),
                    )),
                    "len" => {
                        let n = match vals.first() {
                            Some(Value::Array(a)) => a.borrow().len() as i64,
                            Some(Value::Str(s)) => s.chars().count() as i64,
                            _ => 0,
                        };
                        Ok(Value::Int(n))
                    }
                    "range" => {
                        let (a, b) = match vals.len() {
                            1 => (0i64, vals[0].as_int().unwrap_or(0)),
                            _ => (
                                vals[0].as_int().unwrap_or(0),
                                vals.get(1).and_then(Value::as_int).unwrap_or(0),
                            ),
                        };
                        let items: Vec<Value> = if b > a {
                            (a..b).map(Value::Int).collect()
                        } else {
                            Vec::new()
                        };
                        Ok(Value::Array(Rc::new(RefCell::new(items))))
                    }
                    "Vector3" => Ok(Value::Vec3(Vec3Val::new(
                        vals.first().and_then(Value::num).unwrap_or(0.0) as f32,
                        vals.get(1).and_then(Value::num).unwrap_or(0.0) as f32,
                        vals.get(2).and_then(Value::num).unwrap_or(0.0) as f32,
                    ))),
                    "randf" => Ok(Value::Float(self.rand01())),
                    "randf_range" => {
                        let a = vals.first().and_then(Value::num).unwrap_or(0.0);
                        let b = vals.get(1).and_then(Value::num).unwrap_or(0.0);
                        Ok(Value::Float(a + self.rand01() * (b - a)))
                    }
                    "abs" => match vals.first() {
                        Some(Value::Int(i)) => Ok(Value::Int(i.abs())),
                        Some(Value::Float(f)) => Ok(Value::Float(f.abs())),
                        _ => Ok(Value::Nil),
                    },
                    "floor" => match vals.first().and_then(Value::num) {
                        Some(f) => Ok(Value::Float(f.floor())),
                        None => Ok(Value::Nil),
                    },
                    "sqrt" => match vals.first().and_then(Value::num) {
                        Some(f) => Ok(Value::Float(f.sqrt())),
                        None => Ok(Value::Nil),
                    },
                    "min" => {
                        let m = vals.iter().filter_map(Value::num).fold(f64::INFINITY, f64::min);
                        Ok(Value::Float(m))
                    }
                    "max" => {
                        let m = vals.iter().filter_map(Value::num).fold(f64::NEG_INFINITY, f64::max);
                        Ok(Value::Float(m))
                    }
                    "clamp" => {
                        let v = vals.first().and_then(Value::num).unwrap_or(0.0);
                        let lo = vals.get(1).and_then(Value::num).unwrap_or(0.0);
                        let hi = vals.get(2).and_then(Value::num).unwrap_or(1.0);
                        Ok(Value::Float(v.clamp(lo, hi)))
                    }
                    "sin" => Ok(Value::Float(
                        vals.first().and_then(Value::num).unwrap_or(0.0).sin(),
                    )),
                    "cos" => Ok(Value::Float(
                        vals.first().and_then(Value::num).unwrap_or(0.0).cos(),
                    )),
                    "tan" => Ok(Value::Float(
                        vals.first().and_then(Value::num).unwrap_or(0.0).tan(),
                    )),
                    "atan2" => Ok(Value::Float(
                        match (vals.first().and_then(Value::num), vals.get(1).and_then(Value::num)) {
                            (Some(y), Some(x)) => y.atan2(x),
                            _ => 0.0,
                        },
                    )),
                    "deg_to_rad" => Ok(Value::Float(
                        vals.first().and_then(Value::num).unwrap_or(0.0).to_radians(),
                    )),
                    "rad_to_deg" => Ok(Value::Float(
                        vals.first().and_then(Value::num).unwrap_or(0.0).to_degrees(),
                    )),
                    other => Err(err(format!("unknown function '{other}'"))),
                }
            }
            Expr::Member(base, mname) => {
                let b = self.eval(base, scopes, funcs)?;
                match b {
                    Value::Obj(o) => o.borrow_mut().call(mname, &vals).map_err(err),
                    Value::Class(cname) => {
                        if mname != "new" {
                            return Err(err(format!(
                                "class {cname} only supports .new()"
                            )));
                        }
                        if let Some(mut f) = self.functions.remove(cname.as_str()) {
                            let r = f(self, &vals);
                            self.functions.insert(cname.clone(), f);
                            r.map_err(err)
                        } else {
                            Err(err(format!("no constructor registered for '{cname}'")))
                        }
                    }
                    Value::Vec3(v) => match mname.as_str() {
                        "length" => Ok(Value::Float(v.length() as f64)),
                        "normalized" => Ok(Value::Vec3(v.normalized())),
                        "dot" => {
                            let o = vals.first().and_then(Value::as_vec3).unwrap_or(Vec3Val::ZERO);
                            Ok(Value::Float(v.dot(o) as f64))
                        }
                        "cross" => {
                            let o = vals.first().and_then(Value::as_vec3).unwrap_or(Vec3Val::ZERO);
                            Ok(Value::Vec3(v.cross(o)))
                        }
                        "lerp" => {
                            let o = vals.first().and_then(Value::as_vec3).unwrap_or(Vec3Val::ZERO);
                            let t = vals.get(1).and_then(Value::num).unwrap_or(0.0) as f32;
                            Ok(Value::Vec3(v.lerp(o, t)))
                        }
                        "distance_to" => {
                            let o = vals.first().and_then(Value::as_vec3).unwrap_or(Vec3Val::ZERO);
                            Ok(Value::Float(v.distance_to(o) as f64))
                        }
                        other => Err(err(format!(
                            "Vector3 has no method '{other}'"
                        ))),
                    },
                    Value::Array(a) => match mname.as_str() {
                        "duplicate" => Ok(Value::Array(Rc::new(RefCell::new(a.borrow().clone())))),
                        "append" => {
                            if let Some(v) = vals.first() {
                                a.borrow_mut().push(v.clone());
                            }
                            Ok(Value::Nil)
                        }
                        "clear" => {
                            a.borrow_mut().clear();
                            Ok(Value::Nil)
                        }
                        "size" => Ok(Value::Int(a.borrow().len() as i64)),
                        other => Err(err(format!("Array has no method '{other}'"))),
                    },
                    other => Err(err(format!(
                        "cannot call method '{mname}' on {}",
                        other.type_name()
                    ))),
                }
            }
            other => Err(err(format!("cannot call {other:?}"))),
        }
    }

    fn apply_op(&mut self, op: BinOp, l: Value, r: Value, line: usize) -> Result<Value, String> {
        let terr = |m: String| format!("line {line}: {m}");
        match op {
            BinOp::Eq | BinOp::Neq => {
                let eq = values_equal(&l, &r).map_err(terr)?;
                Ok(Value::Bool(if op == BinOp::Eq { eq } else { !eq }))
            }
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                let ord = match (&l, &r) {
                    (Value::Str(a), Value::Str(b)) => a.as_ref().cmp(b.as_ref()),
                    _ => {
                        let (a, b) = match (l.num(), r.num()) {
                            (Some(a), Some(b)) => (a, b),
                            _ => {
                                return Err(terr(format!(
                                    "cannot compare {} and {}",
                                    l.type_name(),
                                    r.type_name()
                                )))
                            }
                        };
                        a.partial_cmp(&b).ok_or_else(|| {
                            terr("numeric comparison failed".to_string())
                        })?
                    }
                };
                let res = match op {
                    BinOp::Lt => ord == std::cmp::Ordering::Less,
                    BinOp::Le => ord != std::cmp::Ordering::Greater,
                    BinOp::Gt => ord == std::cmp::Ordering::Greater,
                    BinOp::Ge => ord != std::cmp::Ordering::Less,
                    _ => unreachable!(),
                };
                Ok(Value::Bool(res))
            }
            BinOp::Add => {
                if let (Some(a), Some(b)) = (l.num(), r.num()) {
                    if matches!(l, Value::Int(_)) && matches!(r, Value::Int(_)) {
                        return Ok(Value::Int(a as i64 + b as i64));
                    }
                    return Ok(Value::Float(a + b));
                }
                if let Value::Str(s) = &l {
                    return Ok(Value::str(format!("{}{}", s, r.to_display())));
                }
                if let Value::Str(s) = &r {
                    return Ok(Value::str(format!("{}{}", l.to_display(), s)));
                }
                if let (Value::Vec3(a), Value::Vec3(b)) = (&l, &r) {
                    return Ok(Value::Vec3(*a + *b));
                }
                if let (Value::Array(a), Value::Array(b)) = (&l, &r) {
                    let mut out = a.borrow().clone();
                    out.extend(b.borrow().iter().cloned());
                    return Ok(Value::Array(Rc::new(RefCell::new(out))));
                }
                Err(terr(format!(
                    "cannot add {} and {}",
                    l.type_name(),
                    r.type_name()
                )))
            }
            BinOp::Sub => {
                if let (Value::Vec3(a), Value::Vec3(b)) = (&l, &r) {
                    return Ok(Value::Vec3(*a - *b));
                }
                arith(op, l, r).map_err(terr)
            }
            BinOp::Mul => {
                if let (Value::Vec3(a), n) = (&l, r.num()) {
                    if let Some(n) = n {
                        return Ok(Value::Vec3(*a * (n as f32)));
                    }
                }
                if let (n, Value::Vec3(b)) = (l.num(), &r) {
                    if let Some(n) = n {
                        return Ok(Value::Vec3(*b * (n as f32)));
                    }
                }
                arith(op, l, r).map_err(terr)
            }
            BinOp::Div => {
                if let (Value::Vec3(a), n) = (&l, r.num()) {
                    if let Some(n) = n {
                        return Ok(Value::Vec3(*a / (n as f32)));
                    }
                }
                arith(op, l, r).map_err(terr)
            }
            BinOp::Mod => arith(op, l, r).map_err(terr),
            BinOp::And | BinOp::Or => Ok(Value::Bool(l.truthy() || r.truthy())),
        }
    }
}

fn arith(op: BinOp, l: Value, r: Value) -> Result<Value, String> {
    let int_op = matches!(l, Value::Int(_)) && matches!(r, Value::Int(_));
    let (a, b) = match (l.num(), r.num()) {
        (Some(a), Some(b)) => (a, b),
        _ => {
            return Err(format!(
                "cannot apply operator to {} and {}",
                l.type_name(),
                r.type_name()
            ))
        }
    };
    if int_op {
        let (ia, ib) = (a as i64, b as i64);
        Ok(match op {
            BinOp::Add => Value::Int(ia + ib),
            BinOp::Sub => Value::Int(ia - ib),
            BinOp::Mul => Value::Int(ia * ib),
            BinOp::Div => {
                if ib == 0 {
                    Value::Float(0.0)
                } else {
                    Value::Int(ia / ib)
                }
            }
            BinOp::Mod => {
                if ib == 0 {
                    Value::Int(0)
                } else {
                    Value::Int(ia % ib)
                }
            }
            _ => return Err("bad arithmetic op".to_string()),
        })
    } else {
        Ok(match op {
            BinOp::Add => Value::Float(a + b),
            BinOp::Sub => Value::Float(a - b),
            BinOp::Mul => Value::Float(a * b),
            BinOp::Div => {
                if b.abs() < 1e-12 {
                    Value::Float(0.0)
                } else {
                    Value::Float(a / b)
                }
            }
            BinOp::Mod => Value::Float(a % b),
            _ => return Err("bad arithmetic op".to_string()),
        })
    }
}
