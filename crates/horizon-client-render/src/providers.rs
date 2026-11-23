//! Vanilla's `context_int_provider` and `context_float_provider` values (furnace
//! fuel times, brewing times), evaluated from the data pack's JSON.
use crate::items::{ItemStack, Items};
use serde_json::Value;
use std::{collections::HashMap, fs, path::Path};

/// Evaluator for `context_int_provider` / `context_float_provider` values.
pub struct Providers {
    ints: HashMap<String, Value>,
    floats: HashMap<String, Value>,
}
fn load_dir(dir: &Path, namespace: &str, prefix: &str, out: &mut HashMap<String, Value>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        let name = p
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_owned();
        if p.is_dir() {
            load_dir(&p, namespace, &format!("{prefix}{name}/"), out);
        } else if let Ok(bytes) = fs::read(&p)
            && let Ok(v) = serde_json::from_slice(&bytes)
        {
            out.insert(format!("{namespace}:{prefix}{name}"), v);
        }
    }
}
impl Providers {
    pub fn load(root: &Path) -> Self {
        let mut ints = HashMap::new();
        let mut floats = HashMap::new();
        for namespace in std::iter::once("minecraft".to_owned())
            .chain(horizon_items::content_namespaces::namespaces(root))
        {
            load_dir(
                &root.join(format!("data/{namespace}/context_int_provider")),
                &namespace,
                "",
                &mut ints,
            );
            load_dir(
                &root.join(format!("data/{namespace}/context_float_provider")),
                &namespace,
                "",
                &mut floats,
            );
        }
        Self { ints, floats }
    }
    fn eval(v: &Value, fast: bool, table: &HashMap<String, Value>, depth: u32) -> f64 {
        if depth > 16 {
            return 0.0;
        }
        match v {
            Value::Number(n) => n.as_f64().unwrap_or(0.0),
            Value::String(name) => table
                .get(name)
                .map_or(0.0, |v| Self::eval(v, fast, table, depth + 1)),
            Value::Object(o) => {
                let f =
                    |k: &str| Self::eval(o.get(k).unwrap_or(&Value::Null), fast, table, depth + 1);
                match o.get("type").and_then(Value::as_str).unwrap_or("") {
                    "minecraft:div" => {
                        let r = f("right");
                        if r == 0.0 {
                            0.0
                        } else {
                            (f("left") / r).trunc()
                        }
                    }
                    "minecraft:mul" => f("left") * f("right"),
                    "minecraft:add" | "minecraft:sum" => f("left") + f("right"),
                    "minecraft:sub" => f("left") - f("right"),
                    "minecraft:constant" => f("value"),
                    "minecraft:conditional" => {
                        // The only condition in the pinned data: block/fast_cooking.
                        let condition = o.get("condition").and_then(Value::as_str)
                            == Some("minecraft:block/fast_cooking");
                        if condition && fast {
                            f("on_true")
                        } else {
                            f("on_false")
                        }
                    }
                    _ => 0.0,
                }
            }
            _ => 0.0,
        }
    }
    /// A plain `context_int_provider` value or reference (no context conditions).
    pub fn int_value(&self, v: &Value) -> i32 {
        Self::eval(v, false, &self.ints, 0) as i32
    }
    /// A plain `context_float_provider` value or reference; `default` when absent.
    pub fn float_value(&self, v: &Value, default: f32) -> f32 {
        if v.is_null() {
            default
        } else {
            Self::eval(v, false, &self.floats, 0) as f32
        }
    }
    /// Probability of adding at least one layer to a composter at level 1.
    /// Resolve references and weighted branches without consuming the world's RNG.
    pub fn compost_chance(&self, layers: &Value) -> f32 {
        self.layer_chance(layers, 0) as f32
    }
    fn layer_chance(&self, v: &Value, depth: u32) -> f64 {
        if depth > 64 {
            return 0.0;
        }
        let next = |v: &Value| self.layer_chance(v, depth + 1);
        match v {
            Value::Number(n) => {
                if n.as_f64().unwrap_or(0.0) >= 1.0 {
                    1.0
                } else {
                    0.0
                }
            }
            Value::Bool(b) => {
                if *b {
                    1.0
                } else {
                    0.0
                }
            }
            Value::String(id) => self.ints.get(id).map_or(0.0, next),
            Value::Object(_) => match v["type"]
                .as_str()
                .unwrap_or("")
                .trim_start_matches("minecraft:")
            {
                "constant" => next(&v["value"]),
                "weighted_list" => {
                    let mut total = 0.0;
                    let mut positive = 0.0;
                    for entry in v["distribution"].as_array().into_iter().flatten() {
                        let weight = entry["weight"].as_i64().unwrap_or(1).max(0) as f64;
                        total += weight;
                        positive += weight * next(&entry["data"]);
                    }
                    if total > 0.0 { positive / total } else { 0.0 }
                }
                "conditional" => next(if composter_condition(&v["condition"]) {
                    &v["on_true"]
                } else {
                    &v["on_false"]
                }),
                "number_dispatcher" => {
                    let selected = v["cases"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .find(|case| composter_condition(&case["condition"]))
                        .map_or(&v["default"], |case| &case["value"]);
                    next(selected)
                }
                _ => 0.0,
            },
            _ => 0.0,
        }
    }
    #[cfg(test)]
    pub fn empty() -> Self {
        Self {
            ints: HashMap::new(),
            floats: HashMap::new(),
        }
    }
    pub fn burn_time(&self, items: &Items, stack: &ItemStack, fast: bool) -> i32 {
        items
            .get(&stack.item)
            .and_then(|i| i.fuel.as_ref())
            .map_or(0, |(time, _)| Self::eval(time, fast, &self.ints, 0) as i32)
    }
    pub fn speed(&self, items: &Items, stack: &ItemStack, fast: bool) -> f32 {
        items
            .get(&stack.item)
            .and_then(|i| i.fuel.as_ref())
            .map_or(1.0, |(_, speed)| {
                Self::eval(speed, fast, &self.floats, 0) as f32
            })
    }
}

/// The block-interaction context used by compostable layers, at level 1.
fn composter_condition(v: &Value) -> bool {
    if let Some(list) = v.as_array() {
        return list.iter().all(composter_condition);
    }
    match v["type"]
        .as_str()
        .or_else(|| v["condition"].as_str())
        .unwrap_or("")
        .trim_start_matches("minecraft:")
    {
        "match_block" | "block_state_property" => {
            let blocks = v.get("blocks").or_else(|| v.get("block"));
            let blocks_ok = match blocks {
                Some(Value::String(id)) => id == "minecraft:composter",
                Some(Value::Array(ids)) => ids.iter().any(|id| id == "minecraft:composter"),
                None => true,
                _ => false,
            };
            blocks_ok
                && v.get("state")
                    .or_else(|| v.get("properties"))
                    .and_then(Value::as_object)
                    .into_iter()
                    .flatten()
                    .all(|(key, value)| key == "level" && value.as_str() == Some("1"))
        }
        "inverted" => !composter_condition(&v["term"]),
        "all_of" => v["terms"]
            .as_array()
            .is_some_and(|terms| terms.iter().all(composter_condition)),
        "any_of" => v["terms"]
            .as_array()
            .is_some_and(|terms| terms.iter().any(composter_condition)),
        _ => false,
    }
}
