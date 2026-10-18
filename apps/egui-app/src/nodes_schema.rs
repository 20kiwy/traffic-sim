//! Loadable node + property schema (`assets/nodes.json`).
//!
//! Mirrors the Godot editor's class registry: every RoadKit node class
//! with its property list (name, kind, default, range, hint), audited
//! from godot-road-generator's .gd sources. The Inspector renders its
//! widgets FROM this file, and the Scene Tree uses the class layout.

use serde_json::Value;

#[derive(Clone, Debug, PartialEq)]
pub enum PropKind {
    Float,
    Int,
    Bool,
    Enum,
    String,
    Vector3,
    Array,
}

impl PropKind {
    pub fn label(&self) -> &'static str {
        match self {
            PropKind::Float => "float",
            PropKind::Int => "int",
            PropKind::Bool => "bool",
            PropKind::Enum => "enum",
            PropKind::String => "String",
            PropKind::Vector3 => "Vector3",
            PropKind::Array => "Array",
        }
    }
}

#[derive(Clone, Debug)]
pub struct PropDef {
    pub name: String,
    pub kind: PropKind,
    pub options: Vec<String>,
    pub default: String,
    pub min: f64,
    pub max: f64,
    pub unit: String,
    pub hint: String,
}

#[derive(Clone, Debug)]
pub struct NodeDef {
    pub class: String,
    pub description: String,
    pub props: Vec<PropDef>,
}

#[derive(Clone, Debug, Default)]
pub struct NodeSchema {
    pub nodes: Vec<NodeDef>,
    pub scene_root: String,
}

impl NodeSchema {
    /// Parse `nodes.json` from the assets dir; falls back to a minimal
    /// built-in registry if the file is missing or invalid.
    pub fn load(assets_dir: &std::path::Path) -> Self {
        let path = assets_dir.join("nodes.json");
        if let Ok(txt) = std::fs::read_to_string(&path) {
            if let Ok(v) = serde_json::from_str::<Value>(&txt) {
                return Self::from_json(&v);
            }
        }
        Self::fallback()
    }

    fn from_json(v: &Value) -> Self {
        let mut schema = Self { nodes: Vec::new(), scene_root: "RoadManager".into() };
        if let Some(root) = v["scene_tree"]["root"].as_str() {
            schema.scene_root = root.to_string();
        }
        if let Some(arr) = v["nodes"].as_array() {
            for n in arr {
                let class = n["class"].as_str().unwrap_or("").to_string();
                if class.is_empty() {
                    continue;
                }
                let description = n["description"].as_str().unwrap_or("").to_string();
                let mut props = Vec::new();
                if let Some(ps) = n["properties"].as_array() {
                    for p in ps {
                        let kind = match p["kind"].as_str().unwrap_or("") {
                            "float" => PropKind::Float,
                            "int" => PropKind::Int,
                            "bool" => PropKind::Bool,
                            "enum" => PropKind::Enum,
                            "vector3" => PropKind::Vector3,
                            "array" => PropKind::Array,
                            _ => PropKind::String,
                        };
                        props.push(PropDef {
                            name: p["name"].as_str().unwrap_or("").to_string(),
                            kind,
                            options: p["options"]
                                .as_array()
                                .map(|a| a.iter().filter_map(|o| o.as_str().map(String::from)).collect())
                                .unwrap_or_default(),
                            default: match &p["default"] {
                                Value::String(s) => s.clone(),
                                Value::Number(n) => n.to_string(),
                                Value::Bool(b) => b.to_string(),
                                _ => String::new(),
                            },
                            min: p["min"].as_f64().unwrap_or(0.0),
                            max: p["max"].as_f64().unwrap_or(1000.0),
                            unit: p["unit"].as_str().unwrap_or("").to_string(),
                            hint: p["hint"].as_str().unwrap_or("").to_string(),
                        });
                    }
                }
                schema.nodes.push(NodeDef { class, description, props });
            }
        }
        schema
    }

    fn fallback() -> Self {
        Self {
            scene_root: "RoadManager".into(),
            nodes: vec![
                NodeDef {
                    class: "RoadPoint".into(),
                    description: "road cross-section".into(),
                    props: vec![
                        PropDef { name: "x".into(), kind: PropKind::Float, options: vec![], default: "0".into(), min: -1e6, max: 1e6, unit: "m".into(), hint: String::new() },
                        PropDef { name: "lanes".into(), kind: PropKind::Int, options: vec![], default: "2".into(), min: 1.0, max: 12.0, unit: String::new(), hint: String::new() },
                    ],
                },
            ],
        }
    }

    pub fn class(&self, name: &str) -> Option<&NodeDef> {
        self.nodes.iter().find(|n| n.class == name)
    }

    pub fn classes(&self) -> impl Iterator<Item = &NodeDef> {
        self.nodes.iter()
    }
}
