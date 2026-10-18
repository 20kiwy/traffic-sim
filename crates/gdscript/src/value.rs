//! Runtime values + the host-object trait.

use std::cell::RefCell;
use std::rc::Rc;

/// Minimal f32 vector mirroring Godot's Vector3 in scripts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vec3Val {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vec3Val {
    pub fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    pub fn length(&self) -> f32 {
        (self.x * self.x + self.y * self.y + self.z * self.z).sqrt()
    }

    pub fn normalized(&self) -> Vec3Val {
        let l = self.length();
        if l > 1e-9 {
            Vec3Val::new(self.x / l, self.y / l, self.z / l)
        } else {
            Vec3Val::ZERO
        }
    }

    pub const ZERO: Vec3Val = Vec3Val { x: 0.0, y: 0.0, z: 0.0 };

    pub fn dot(&self, o: Vec3Val) -> f32 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }

    pub fn cross(&self, o: Vec3Val) -> Vec3Val {
        Vec3Val::new(
            self.y * o.z - self.z * o.y,
            self.z * o.x - self.x * o.z,
            self.x * o.y - self.y * o.x,
        )
    }

    pub fn lerp(&self, o: Vec3Val, t: f32) -> Vec3Val {
        Vec3Val::new(
            self.x + (o.x - self.x) * t,
            self.y + (o.y - self.y) * t,
            self.z + (o.z - self.z) * t,
        )
    }

    pub fn distance_to(&self, o: Vec3Val) -> f32 {
        (*self - o).length()
    }
}

impl std::ops::Add for Vec3Val {
    type Output = Vec3Val;
    fn add(self, o: Vec3Val) -> Vec3Val {
        Vec3Val::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}

impl std::ops::Sub for Vec3Val {
    type Output = Vec3Val;
    fn sub(self, o: Vec3Val) -> Vec3Val {
        Vec3Val::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}

impl std::ops::Mul<f32> for Vec3Val {
    type Output = Vec3Val;
    fn mul(self, s: f32) -> Vec3Val {
        Vec3Val::new(self.x * s, self.y * s, self.z * s)
    }
}

impl std::ops::Div<f32> for Vec3Val {
    type Output = Vec3Val;
    fn div(self, s: f32) -> Vec3Val {
        if s.abs() > 1e-12 {
            Vec3Val::new(self.x / s, self.y / s, self.z / s)
        } else {
            Vec3Val::ZERO
        }
    }
}

/// Objects exposed to scripts by the embedder (RoadPoint, RoadCurve, ...).
pub trait GodotObject {
    fn class_name(&self) -> &'static str;
    fn get(&self, name: &str) -> Option<Value>;
    fn set(&mut self, name: &str, v: Value) -> Result<(), String>;
    fn call(&mut self, name: &str, args: &[Value]) -> Result<Value, String>;
    fn describe(&self) -> String {
        format!("<{}>", self.class_name())
    }
}

#[derive(Clone)]
pub enum Value {
    Nil,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(Rc<String>),
    Vec3(Vec3Val),
    Array(Rc<RefCell<Vec<Value>>>),
    Obj(Rc<RefCell<dyn GodotObject>>),
    /// a registered constructor class awaiting `.new()` (e.g. RoadCurve)
    Class(String),
}

impl std::fmt::Debug for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_display())
    }
}

impl Value {
    pub fn str(s: impl Into<String>) -> Value {
        Value::Str(Rc::new(s.into()))
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Nil => "nil",
            Value::Bool(_) => "bool",
            Value::Int(_) => "int",
            Value::Float(_) => "float",
            Value::Str(_) => "String",
            Value::Vec3(_) => "Vector3",
            Value::Array(_) => "Array",
            Value::Obj(o) => match o.try_borrow() {
                Ok(o) => o.class_name(),
                Err(_) => "<object>",
            },
            Value::Class(_) => "class",
        }
    }

    pub fn truthy(&self) -> bool {
        match self {
            Value::Nil => false,
            Value::Bool(b) => *b,
            Value::Int(i) => *i != 0,
            Value::Float(f) => *f != 0.0,
            Value::Str(s) => !s.is_empty(),
            Value::Vec3(v) => v.length() > 0.0,
            Value::Array(a) => !a.borrow().is_empty(),
            Value::Obj(_) | Value::Class(_) => true,
        }
    }

    pub fn num(&self) -> Option<f64> {
        match self {
            Value::Int(i) => Some(*i as f64),
            Value::Float(f) => Some(*f),
            _ => None,
        }
    }

    pub fn as_vec3(&self) -> Option<Vec3Val> {
        match self {
            Value::Vec3(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_int(&self) -> Option<i64> {
        match self {
            Value::Int(i) => Some(*i),
            Value::Float(f) => Some(*f as i64),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<String> {
        match self {
            Value::Str(s) => Some(s.as_ref().clone()),
            _ => None,
        }
    }

    /// Human display (used by print / str()).
    pub fn to_display(&self) -> String {
        match self {
            Value::Nil => "null".to_string(),
            Value::Bool(b) => if *b { "true" } else { "false" }.to_string(),
            Value::Int(i) => i.to_string(),
            Value::Float(f) => fmt_float(*f),
            Value::Str(s) => s.as_ref().clone(),
            Value::Vec3(v) => format!(
                "Vector3({}, {}, {})",
                fmt_float(v.x as f64),
                fmt_float(v.y as f64),
                fmt_float(v.z as f64)
            ),
            Value::Array(a) => {
                let items: Vec<String> = a.borrow().iter().map(Value::to_display).collect();
                format!("[{}]", items.join(", "))
            }
            Value::Obj(o) => o.borrow().describe(),
            Value::Class(c) => format!("<class {c}>"),
        }
    }
}

pub fn fmt_float(f: f64) -> String {
    if f == f.trunc() && f.abs() < 1e15 {
        format!("{f:.1}")
    } else {
        let s = format!("{f:.4}");
        let t = s.trim_end_matches('0').trim_end_matches('.');
        t.to_string()
    }
}

pub fn values_equal(a: &Value, b: &Value) -> Result<bool, String> {
    Ok(match (a, b) {
        (Value::Nil, Value::Nil) => true,
        (Value::Nil, _) | (_, Value::Nil) => false,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Int(x), Value::Int(y)) => x == y,
        (Value::Float(x), Value::Float(y)) => x == y,
        (Value::Int(x), Value::Float(y)) | (Value::Float(y), Value::Int(x)) => (*x as f64) == *y,
        (Value::Str(x), Value::Str(y)) => x == y,
        (Value::Vec3(x), Value::Vec3(y)) => x == y,
        (Value::Array(x), Value::Array(y)) => Rc::ptr_eq(x, y),
        (Value::Obj(x), Value::Obj(y)) => Rc::ptr_eq(x, y),
        (Value::Class(x), Value::Class(y)) => x == y,
        _ => false,
    })
}
