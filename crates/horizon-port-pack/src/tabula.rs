//! Tabula models (`.tbl`: a zip of `model.json` and a texture, as the Citadel library reads
//! them for Ice and Fire and others) carried over as Bedrock geometry and animations, the
//! format a content mod entity's own model takes (docs/modpacks.md, entities).
//!
//! A Tabula cube is a `ModelPart` in vanilla's model space: a rotation point relative to its
//! parent's, a box offset from it, a texture offset, a rotation in degrees. A point (x, y, z)
//! of that space is (x, 24 - y, z) of Bedrock's, rotations keep their signs, and a pose
//! model's differences from the base model are an animation that holds them.
use serde::Serialize;
use serde_json::{Map, Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Read, Seek},
    path::Path,
};

/// What carrying a mod's Tabula models did.
#[derive(Debug, Default, Serialize)]
pub struct Report {
    /// `.tbl` files written as `assets/<ns>/geo/tabula/<path>.geo.json`.
    pub models: usize,
    /// Animation files written beside a folder's base models, and the base models they belong
    /// to (`assets/<ns>/animations/tabula/<path>.animation.json`).
    pub animation_files: Vec<String>,
    /// Files that could not be read, and why.
    pub unreadable: BTreeMap<String, String>,
    /// What the conversion drops (cube scales, model scales), by file.
    pub dropped: BTreeMap<String, Vec<String>>,
}

/// The `model.json` of a `.tbl` file.
pub fn read(bytes: &[u8]) -> Result<Value, String> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let mut json = Vec::new();
    zip.by_name("model.json")
        .map_err(|e| format!("model.json: {e}"))?
        .read_to_end(&mut json)
        .map_err(|e| e.to_string())?;
    serde_json::from_slice(&json).map_err(|e| format!("model.json: {e}"))
}

/// One cube of the model, flattened: its bone name (unique), its parent's, and its pose.
#[derive(Clone, Debug)]
struct Cube<'a> {
    name: String,
    parent: Option<String>,
    /// The rotation point in the model space, its parents' added.
    pivot: [f64; 3],
    /// The rotation point relative to the parent's, as the file has it.
    position: [f64; 3],
    rotation: [f64; 3],
    v: &'a Value,
}

fn v3(v: &Value, default: f64) -> [f64; 3] {
    std::array::from_fn(|i| v[i].as_f64().unwrap_or(default))
}

/// Every cube, parents before children, named uniquely (a repeated name gets `_2`, `_3`...).
fn cubes(model: &Value) -> Vec<Cube<'_>> {
    fn walk<'a>(
        v: &'a Value,
        parent: Option<(&str, [f64; 3])>,
        used: &mut BTreeSet<String>,
        out: &mut Vec<Cube<'a>>,
    ) {
        let base = v["name"].as_str().unwrap_or("cube").to_owned();
        let mut name = base.clone();
        let mut n = 1;
        while !used.insert(name.clone()) {
            n += 1;
            name = format!("{base}_{n}");
        }
        let position = v3(&v["position"], 0.0);
        let origin = parent.map_or([0.0; 3], |(_, p)| p);
        let pivot = std::array::from_fn(|i| origin[i] + position[i]);
        out.push(Cube {
            name: name.clone(),
            parent: parent.map(|(p, _)| p.to_owned()),
            pivot,
            position,
            rotation: v3(&v["rotation"], 0.0),
            v,
        });
        for child in v["children"].as_array().into_iter().flatten() {
            walk(child, Some((&name, pivot)), used, out);
        }
    }
    let mut used = BTreeSet::new();
    let mut out = Vec::new();
    let groups = model["cubeGroups"].as_array().into_iter().flatten();
    let grouped = groups.flat_map(group_cubes);
    for c in model["cubes"]
        .as_array()
        .into_iter()
        .flatten()
        .chain(grouped)
    {
        walk(c, None, &mut used, &mut out);
    }
    out
}
/// A cube group's cubes and its subgroups', which sit at the model's root.
fn group_cubes(g: &Value) -> Vec<&Value> {
    let mut out: Vec<&Value> = g["cubes"].as_array().into_iter().flatten().collect();
    for sub in g["cubeGroups"].as_array().into_iter().flatten() {
        out.extend(group_cubes(sub));
    }
    out
}

fn round(v: f64) -> Value {
    json!((v * 10_000.0).round() / 10_000.0)
}
fn vec3(v: [f64; 3]) -> Value {
    Value::Array(v.iter().map(|x| round(*x)).collect())
}

/// The model as Bedrock geometry (format 1.12): a bone per cube, named after it. `dropped`
/// gets what Bedrock geometry cannot hold.
pub fn geometry(model: &Value, identifier: &str, dropped: &mut Vec<String>) -> Value {
    let texture = [
        model["textureWidth"].as_u64().unwrap_or(64),
        model["textureHeight"].as_u64().unwrap_or(32),
    ];
    if v3(&model["scale"], 1.0) != [1.0; 3] {
        dropped.push(format!("the model's scale {}", model["scale"]));
    }
    let bones: Vec<Value> = cubes(model)
        .iter()
        .map(|c| {
            let v = c.v;
            let [x, y, z] = c.pivot;
            let mut bone = Map::new();
            bone.insert("name".into(), json!(c.name));
            if let Some(p) = &c.parent {
                bone.insert("parent".into(), json!(p));
            }
            bone.insert("pivot".into(), vec3([x, 24.0 - y, z]));
            if c.rotation != [0.0; 3] {
                bone.insert("rotation".into(), vec3(c.rotation));
            }
            if v3(&v["scale"], 1.0) != [1.0; 3] {
                dropped.push(format!("{}'s scale {}", c.name, v["scale"]));
            }
            if !v["hidden"].as_bool().unwrap_or(false) {
                let size = v3(&v["dimensions"], 0.0);
                let offset = v3(&v["offset"], 0.0);
                let min: [f64; 3] = std::array::from_fn(|i| c.pivot[i] + offset[i]);
                let mut cube = Map::new();
                cube.insert(
                    "origin".into(),
                    vec3([min[0], 24.0 - min[1] - size[1], min[2]]),
                );
                cube.insert("size".into(), vec3(size));
                cube.insert("uv".into(), json!([v["txOffset"][0], v["txOffset"][1]]));
                let grow = v["mcScale"].as_f64().unwrap_or(0.0);
                if grow != 0.0 {
                    cube.insert("inflate".into(), round(grow));
                }
                if v["txMirror"].as_bool().unwrap_or(false) {
                    cube.insert("mirror".into(), json!(true));
                }
                bone.insert("cubes".into(), json!([cube]));
            }
            Value::Object(bone)
        })
        .collect();
    json!({
        "format_version": "1.12.0",
        "minecraft:geometry": [{
            "description": {
                "identifier": format!("geometry.{identifier}"),
                "texture_width": texture[0],
                "texture_height": texture[1],
            },
            "bones": bones,
        }],
    })
}

/// Each bone's offsets from `base` to `pose` (by bone name): rotation in degrees and position in
/// Bedrock's axes (y up), the ones that differ.
fn offsets(base: &Value, pose: &Value) -> BTreeMap<String, ([f64; 3], [f64; 3])> {
    let base: BTreeMap<String, Cube> = cubes(base)
        .into_iter()
        .map(|c| (c.name.clone(), c))
        .collect();
    let mut out = BTreeMap::new();
    for c in cubes(pose) {
        let Some(b) = base.get(&c.name) else {
            continue;
        };
        let r: [f64; 3] = std::array::from_fn(|i| c.rotation[i] - b.rotation[i]);
        let p: [f64; 3] = std::array::from_fn(|i| c.position[i] - b.position[i]);
        let r = r.map(|v| if v.abs() < 1e-4 { 0.0 } else { v });
        let p = p.map(|v| if v.abs() < 1e-4 { 0.0 } else { v });
        if r != [0.0; 3] || p != [0.0; 3] {
            out.insert(c.name, (r, [p[0], -p[1], p[2]]));
        }
    }
    out
}

/// `pose` held as an animation of `base`: each bone that differs, turned and moved there.
pub fn pose(base: &Value, pose: &Value) -> Value {
    let bones: Map<String, Value> = offsets(base, pose)
        .into_iter()
        .map(|(name, (r, p))| (name, channels(&[(0.0, r, p)])))
        .collect();
    json!({"loop": "hold_on_last_frame", "animation_length": 0.0, "bones": bones})
}

/// `poses` in turn, `seconds` apart, back to the first and around again.
pub fn sequence(base: &Value, poses: &[&Value], seconds: f64) -> Value {
    let frames: Vec<BTreeMap<String, ([f64; 3], [f64; 3])>> =
        poses.iter().map(|p| offsets(base, p)).collect();
    let names: BTreeSet<&String> = frames.iter().flat_map(|f| f.keys()).collect();
    let n = frames.len();
    let bones: Map<String, Value> = names
        .into_iter()
        .map(|name| {
            let keys: Vec<(f64, [f64; 3], [f64; 3])> = (0..=n)
                .map(|i| {
                    let (r, p) = frames[i % n].get(name).copied().unwrap_or_default();
                    (i as f64 * seconds, r, p)
                })
                .collect();
            (name.clone(), channels(&keys))
        })
        .collect();
    json!({"loop": true, "animation_length": round(n as f64 * seconds), "bones": bones})
}

fn channels(keys: &[(f64, [f64; 3], [f64; 3])]) -> Value {
    let channel = |pick: &dyn Fn(&(f64, [f64; 3], [f64; 3])) -> [f64; 3]| {
        if keys.iter().all(|k| pick(k) == [0.0; 3]) {
            return None;
        }
        let frames: Map<String, Value> = keys
            .iter()
            .map(|k| {
                (
                    format!("{}", (k.0 * 10_000.0).round() / 10_000.0),
                    vec3(pick(k)),
                )
            })
            .collect();
        Some(Value::Object(frames))
    };
    let mut out = Map::new();
    if let Some(r) = channel(&|k| k.1) {
        out.insert("rotation".into(), r);
    }
    if let Some(p) = channel(&|k| k.2) {
        out.insert("position".into(), p);
    }
    Value::Object(out)
}

/// A folder's base models: those named `base`, `ground`, `default` or `idle`, else the first.
fn bases(stems: &[&str]) -> Vec<String> {
    let named: Vec<String> = stems
        .iter()
        .filter(|s| {
            ["base", "ground", "default", "idle"]
                .iter()
                .any(|w| s.contains(w))
        })
        .map(|s| s.to_string())
        .collect();
    if named.is_empty() {
        stems
            .first()
            .map(|s| vec![s.to_string()])
            .unwrap_or_default()
    } else {
        named
    }
}

/// A stem ending in a number and the stem without it (`firedragon_flight3` → `firedragon_flight`, 3).
fn numbered(stem: &str) -> Option<(&str, u32)> {
    let digits = stem.len() - stem.trim_end_matches(|c: char| c.is_ascii_digit()).len();
    let (head, n) = stem.split_at(stem.len() - digits);
    Some((head.trim_end_matches('_'), n.parse().ok()?))
}

/// The seconds between the poses of a numbered run (`flight1`, `flight2`...).
pub const SEQUENCE_SECONDS: f64 = 0.25;

/// Writes every `.tbl` under `assets/<ns>/` of the mod's namespaces as Bedrock geometry under
/// `out/assets/<ns>/geo/tabula/`, and for each folder's base models an animation file beside
/// them under `animations/tabula/`: every other model of the folder as a held pose
/// (`animation.<stem>`), and each numbered run as a loop (`animation.<run>`).
pub fn carry<R: Read + Seek>(
    zip: &mut zip::ZipArchive<R>,
    namespaces: &[&str],
    out: &Path,
) -> Result<Report, String> {
    let mut report = Report::default();
    // Folder (`<ns>/<dir>`) -> stem -> model.
    let mut folders: BTreeMap<(String, String), BTreeMap<String, Value>> = BTreeMap::new();
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
        let name = entry.name().to_owned();
        let Some(rest) = name.strip_prefix("assets/").filter(|n| n.ends_with(".tbl")) else {
            continue;
        };
        let Some((ns, path)) = rest.split_once('/') else {
            continue;
        };
        if !namespaces.contains(&ns) || name.split('/').any(|p| p == ".." || p.is_empty()) {
            continue;
        }
        let mut bytes = Vec::new();
        entry
            .read_to_end(&mut bytes)
            .map_err(|e| format!("{name}: {e}"))?;
        let model = match read(&bytes) {
            Ok(m) => m,
            Err(e) => {
                report.unreadable.insert(name, e);
                continue;
            }
        };
        // `models/tabula/firedragon/x.tbl` → folder `firedragon`, stem `x`.
        let path = path.trim_end_matches(".tbl");
        let path = path.strip_prefix("models/").unwrap_or(path);
        let path = path.strip_prefix("tabula/").unwrap_or(path).to_lowercase();
        let (dir, stem) = path.rsplit_once('/').unwrap_or(("", &path));
        folders
            .entry((ns.to_owned(), dir.to_owned()))
            .or_default()
            .insert(stem.to_owned(), model);
    }
    let write = |rel: String, v: &Value| -> Result<(), String> {
        let path = out.join(&rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{rel}: {e}"))?;
        }
        let bytes = serde_json::to_vec_pretty(v).map_err(|e| e.to_string())?;
        std::fs::write(&path, bytes).map_err(|e| format!("{rel}: {e}"))
    };
    for ((ns, dir), models) in &folders {
        let at = |stem: &str| {
            if dir.is_empty() {
                stem.to_owned()
            } else {
                format!("{dir}/{stem}")
            }
        };
        for (stem, model) in models {
            let mut dropped = Vec::new();
            let geo = geometry(model, &at(stem).replace('/', "."), &mut dropped);
            write(
                format!("assets/{ns}/geo/tabula/{}.geo.json", at(stem)),
                &geo,
            )?;
            report.models += 1;
            if !dropped.is_empty() {
                report.dropped.insert(format!("{ns}:{}", at(stem)), dropped);
            }
        }
        if models.len() < 2 {
            continue;
        }
        let stems: Vec<&str> = models.keys().map(String::as_str).collect();
        let mut runs: BTreeMap<&str, Vec<(u32, &str)>> = BTreeMap::new();
        for s in &stems {
            if let Some((run, n)) = numbered(s) {
                runs.entry(run).or_default().push((n, s));
            }
        }
        for base in bases(&stems) {
            let b = &models[&base];
            let mut animations = Map::new();
            for (stem, model) in models.iter().filter(|(s, _)| **s != base) {
                animations.insert(format!("animation.{stem}"), pose(b, model));
            }
            for (run, mut poses) in runs.clone().into_iter().filter(|(_, p)| p.len() > 1) {
                poses.sort();
                let poses: Vec<&Value> = poses.iter().map(|(_, s)| &models[*s]).collect();
                animations.insert(
                    format!("animation.{run}"),
                    sequence(b, &poses, SEQUENCE_SECONDS),
                );
            }
            let rel = format!("assets/{ns}/animations/tabula/{}.animation.json", at(&base));
            write(
                rel.clone(),
                &json!({"format_version": "1.8.0", "animations": animations}),
            )?;
            report.animation_files.push(rel);
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// A neck with a head, as Tabula saves a `ModelPart` tree.
    fn model(head_x: f64, neck_y: f64) -> Value {
        json!({
            "modelName": "Test", "textureWidth": 64, "textureHeight": 32, "scale": [1.0, 1.0, 1.0],
            "cubeGroups": [],
            "cubes": [{
                "name": "Neck", "dimensions": [2, 4, 2], "position": [0.0, neck_y, -4.0],
                "offset": [-1.0, -4.0, -1.0], "rotation": [10.0, 0.0, 0.0], "scale": [1.0, 1.0, 1.0],
                "txOffset": [0, 0], "txMirror": false, "mcScale": 0.0, "hidden": false,
                "children": [{
                    "name": "Head", "dimensions": [4, 4, 6], "position": [0.0, -4.0, 0.0],
                    "offset": [-2.0, -2.0, -6.0], "rotation": [head_x, 0.0, 0.0],
                    "scale": [1.0, 1.0, 1.0], "txOffset": [8, 0], "txMirror": true, "mcScale": 0.5,
                    "hidden": false, "children": []
                }]
            }]
        })
    }

    #[test]
    fn cubes_become_bones_in_bedrock_space() {
        let mut dropped = Vec::new();
        let geo = geometry(&model(-20.0, 10.0), "test", &mut dropped);
        assert!(dropped.is_empty());
        let g = &geo["minecraft:geometry"][0];
        assert_eq!(g["description"]["texture_height"], 32);
        let (neck, head) = (&g["bones"][0], &g["bones"][1]);
        assert_eq!(neck["name"], "Neck");
        assert_eq!(neck["pivot"], json!([0.0, 14.0, -4.0]));
        assert_eq!(neck["rotation"], json!([10.0, 0.0, 0.0]));
        // Box at y 6..10 in vanilla's space: Bedrock y 14..18.
        assert_eq!(neck["cubes"][0]["origin"], json!([-1.0, 14.0, -5.0]));
        assert_eq!(head["parent"], "Neck");
        assert_eq!(head["pivot"], json!([0.0, 18.0, -4.0]));
        assert_eq!(head["cubes"][0]["inflate"], json!(0.5));
        assert_eq!(head["cubes"][0]["mirror"], json!(true));
        assert_eq!(head["cubes"][0]["uv"], json!([8, 0]));
    }

    /// A pose holds each bone's differences from the base; a numbered run loops through its poses.
    #[test]
    fn a_pose_is_held_and_a_run_loops() {
        let base = model(0.0, 10.0);
        let p = pose(&base, &model(-30.0, 8.0));
        assert_eq!(
            p["bones"]["Head"]["rotation"]["0"],
            json!([-30.0, 0.0, 0.0])
        );
        // Up two pixels in vanilla's space is up two in Bedrock's.
        assert_eq!(p["bones"]["Neck"]["position"]["0"], json!([0.0, 2.0, 0.0]));
        assert!(p["bones"]["Neck"].get("rotation").is_none());
        let (a, b) = (model(10.0, 10.0), model(20.0, 10.0));
        let s = sequence(&base, &[&a, &b], 0.25);
        assert_eq!(s["animation_length"], json!(0.5));
        let head = &s["bones"]["Head"]["rotation"];
        assert_eq!(head["0"], json!([10.0, 0.0, 0.0]));
        assert_eq!(head["0.25"], json!([20.0, 0.0, 0.0]));
        assert_eq!(head["0.5"], json!([10.0, 0.0, 0.0]));
    }

    #[test]
    fn names_runs_and_bases() {
        assert_eq!(
            numbered("firedragon_flight3"),
            Some(("firedragon_flight", 3))
        );
        assert_eq!(numbered("firedragon_ground"), None);
        assert_eq!(
            bases(&["a_bite1", "a_ground", "b_base_male"]),
            ["a_ground", "b_base_male"]
        );
        assert_eq!(bases(&["x", "y"]), ["x"]);
        let twice = json!({"cubes": [{"name": "Leg"}, {"name": "Leg"}]});
        let names: Vec<String> = cubes(&twice).into_iter().map(|c| c.name).collect();
        assert_eq!(names, ["Leg", "Leg_2"]);
    }

    fn tbl(model: &Value) -> Vec<u8> {
        let mut out = std::io::Cursor::new(Vec::new());
        let mut w = zip::ZipWriter::new(&mut out);
        w.start_file("model.json", zip::write::SimpleFileOptions::default())
            .unwrap();
        w.write_all(model.to_string().as_bytes()).unwrap();
        w.finish().unwrap();
        out.into_inner()
    }

    #[test]
    fn carries_a_folder_of_models() {
        let mut jar = std::io::Cursor::new(Vec::new());
        let mut w = zip::ZipWriter::new(&mut jar);
        let opts = zip::write::SimpleFileOptions::default();
        for (name, m) in [
            ("ruby/models/tabula/wyrm/wyrm_ground.tbl", model(0.0, 10.0)),
            ("ruby/models/tabula/wyrm/wyrm_fly1.tbl", model(10.0, 10.0)),
            ("ruby/models/tabula/wyrm/wyrm_fly2.tbl", model(20.0, 10.0)),
            ("other/models/tabula/x.tbl", model(0.0, 0.0)),
        ] {
            w.start_file(format!("assets/{name}"), opts).unwrap();
            w.write_all(&tbl(&m)).unwrap();
        }
        w.start_file("assets/ruby/models/tabula/broken.tbl", opts)
            .unwrap();
        w.write_all(b"not a zip").unwrap();
        w.finish().unwrap();
        let out = std::env::temp_dir().join(format!("horizon-tabula-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&out);
        let mut zip = zip::ZipArchive::new(jar).unwrap();
        let report = carry(&mut zip, &["ruby"], &out).unwrap();
        assert_eq!(report.models, 3);
        assert!(
            report
                .unreadable
                .contains_key("assets/ruby/models/tabula/broken.tbl")
        );
        assert!(
            out.join("assets/ruby/geo/tabula/wyrm/wyrm_fly1.geo.json")
                .is_file()
        );
        assert!(!out.join("assets/other").exists());
        assert_eq!(
            report.animation_files,
            ["assets/ruby/animations/tabula/wyrm/wyrm_ground.animation.json"]
        );
        let anims: Value =
            serde_json::from_slice(&std::fs::read(out.join(&report.animation_files[0])).unwrap())
                .unwrap();
        let names: Vec<&String> = anims["animations"].as_object().unwrap().keys().collect();
        assert_eq!(
            names,
            [
                "animation.wyrm_fly",
                "animation.wyrm_fly1",
                "animation.wyrm_fly2"
            ]
        );
        let _ = std::fs::remove_dir_all(&out);
    }
}
