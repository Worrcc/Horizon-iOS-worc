//! Bedrock entity geometry (`*.geo.json`, the format Blockbench exports for GeckoLib) baked
//! into model parts: a content mod entity's own model (docs/modpacks.md, entities).
//!
//! Bedrock models are y-up with absolute pivots, and GeckoLib draws them with x negated; vanilla's
//! are y-down, drawn under `scale(-1, -1, 1)` and a 1.501 drop. A point (x, y, z) of the one is
//! (x, 24 - y, z) of the other, and a bone's rotation keeps its signs, as vanilla's
//! `KeyframeAnimations.degreeVec` does for animations made in the same tools.
use super::{Mob, Part, Polygon, anim::Poses, anim::State, keyframes::Animation};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

/// The layer key of a content mod entity's own model in its type's layers.
pub const MODEL_LAYER: &str = "horizon#model";

/// What a mod entity's own animations play: one of the moving or standing loops, and the
/// attack over them while it swings (as GeckoLib mods commonly wire their controllers), or
/// what a server mod's `animate` order names (mobs/orders.rs).
#[derive(Clone, Debug, Default)]
pub struct Animations {
    pub idle: Option<Animation>,
    pub walk: Option<Animation>,
    pub attack: Option<Animation>,
    /// Every animation of the model's file, by name.
    pub all: HashMap<String, Animation>,
    /// Those that hold their last frame when they end (`"loop": "hold_on_last_frame"`).
    pub holds: HashSet<String>,
}

/// A mod entity's model and animations from the content root: the geometry, and when the
/// model has an animation file, its animations and the states' choice of them.
pub fn load(
    root: &Path,
    model: &str,
    states: &serde_json::Map<String, Value>,
) -> Result<(Part, Option<Animations>), String> {
    let (ns, path) = model
        .split_once(':')
        .ok_or("a model is <namespace>:<path>")?;
    let read = |file: &Path| -> Result<Value, String> {
        let bytes = std::fs::read(file).map_err(|e| format!("{}: {e}", file.display()))?;
        serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", file.display()))
    };
    let part = parse(&read(
        &root.join(format!("assets/{ns}/geo/{path}.geo.json")),
    )?)?;
    let file = root.join(format!("assets/{ns}/animations/{path}.animation.json"));
    if states.is_empty() && !file.is_file() {
        return Ok((part, None));
    }
    let json = read(&file)?;
    let all = super::bedrock_anim::parse(&json)?;
    let holds = json["animations"]
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(_, a)| a["loop"] == "hold_on_last_frame")
        .map(|(name, _)| name.clone())
        .collect();
    let take = |state: &str| -> Result<Option<Animation>, String> {
        let Some(name) = states.get(state).and_then(Value::as_str) else {
            return Ok(None);
        };
        all.get(name)
            .cloned()
            .map(Some)
            .ok_or_else(|| format!("no animation {name} for {state}"))
    };
    let animations = Animations {
        idle: take("idle")?,
        walk: take("walk")?,
        attack: take("attack")?,
        all,
        holds,
    };
    Ok((part, Some(animations)))
}

/// A mod entity's pose for the frame: its head turned to where it looks, then the animation
/// an order plays (once, or looping, or held at its end as its file says), or else the walk
/// loop while its limbs swing (GeckoLib's `isMoving`, beyond 0.15) or the idle one, both in
/// real time, and the attack through its swing.
pub fn animate(m: &Mob, s: &State, p: &mut Poses, a: &Animations) {
    if p.has("head") {
        let head = p.get("head");
        head[3] += s.x_rot.to_radians();
        head[4] += s.y_rot.to_radians();
    }
    if let Some((name, start)) = &m.orders.animation
        && let Some(ordered) = a.all.get(&**name)
    {
        let millis = ((s.age - start).max(0.0) * 50.0) as i64;
        let end = (ordered.length * 1000.0) as i64;
        if ordered.looping || millis <= end {
            return ordered.apply(millis, 1.0, p);
        }
        if a.holds.contains(&**name) {
            return ordered.apply(end, 1.0, p);
        }
    }
    let millis = (s.age * 50.0) as i64;
    let moving = s.speed > 0.15;
    if let Some(walk) = a.walk.as_ref().filter(|_| moving) {
        walk.apply(millis, 1.0, p);
    } else if let Some(idle) = &a.idle {
        idle.apply(millis, 1.0, p);
    }
    if let Some(attack) = a.attack.as_ref().filter(|_| s.swing > 0.0 && m.alive()) {
        attack.apply((s.swing * attack.length * 1000.0) as i64, 1.0, p);
    }
}

/// The first geometry of a `*.geo.json` file: a root part holding its root bones.
pub fn parse(file: &Value) -> Result<Part, String> {
    let (geometry, texture) = geometry(file).ok_or("no geometry in the file")?;
    let bones: Vec<&Value> = geometry["bones"].as_array().into_iter().flatten().collect();
    let names: Vec<&str> = bones
        .iter()
        .map(|b| b["name"].as_str().unwrap_or(""))
        .collect();
    if names.iter().any(|n| n.is_empty()) {
        return Err("a bone without a name".into());
    }
    if let Some(n) = names
        .iter()
        .enumerate()
        .find_map(|(i, n)| names[..i].contains(n).then_some(n))
    {
        return Err(format!("two bones named {n}"));
    }
    let parent = |b: &Value| {
        b["parent"]
            .as_str()
            .and_then(|p| names.iter().position(|n| *n == p))
    };
    let mut root = Part {
        // Part::parse's rule: an inner bone named "root" keeps the name.
        name: if names.contains(&"root") { "" } else { "root" }.to_owned(),
        pose: REST,
        polygons: Vec::new(),
        children: Vec::new(),
    };
    for (i, b) in bones.iter().enumerate() {
        if parent(b).is_none() {
            let mut seen = vec![i];
            root.children
                .push(bone(i, &bones, &parent, [0.0; 3], texture, &mut seen)?);
        }
    }
    if root.children.is_empty() && !bones.is_empty() {
        return Err("every bone has a parent: a cycle".into());
    }
    root.children.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(root)
}

const REST: [f32; 9] = [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0];

/// The geometry object and its texture size: format 1.12's `minecraft:geometry` list, or
/// 1.8's `geometry.<name>` keys.
fn geometry(file: &Value) -> Option<(&Value, [f32; 2])> {
    if let Some(g) = file["minecraft:geometry"]
        .as_array()
        .and_then(|a| a.first())
    {
        let d = &g["description"];
        return Some((g, size(&d["texture_width"], &d["texture_height"])));
    }
    let (_, g) = file
        .as_object()?
        .iter()
        .find(|(k, _)| k.starts_with("geometry."))?;
    Some((g, size(&g["texturewidth"], &g["textureheight"])))
}
fn size(w: &Value, h: &Value) -> [f32; 2] {
    let f = |v: &Value| v.as_f64().filter(|v| *v > 0.0).unwrap_or(64.0) as f32;
    [f(w), f(h)]
}
fn v3(v: &Value, default: [f32; 3]) -> [f32; 3] {
    match v.as_array() {
        Some(a) if a.len() == 3 => std::array::from_fn(|i| a[i].as_f64().unwrap_or(0.0) as f32),
        _ => default,
    }
}
/// A Bedrock position in vanilla's model space.
fn point(p: [f32; 3]) -> glam::Vec3 {
    glam::Vec3::new(p[0], 24.0 - p[1], p[2])
}
/// `ModelPart`'s rotation order, `rotationZYX(z, y, x)`, from Bedrock degrees.
fn rotation(r: [f32; 3]) -> glam::Mat3 {
    glam::Mat3::from_euler(
        glam::EulerRot::ZYX,
        r[2].to_radians(),
        r[1].to_radians(),
        r[0].to_radians(),
    )
}

fn bone(
    index: usize,
    bones: &[&Value],
    parent: &dyn Fn(&Value) -> Option<usize>,
    parent_pivot: [f32; 3],
    texture: [f32; 2],
    seen: &mut Vec<usize>,
) -> Result<Part, String> {
    let b = bones[index];
    let pivot = point(v3(&b["pivot"], [0.0; 3]));
    let r = v3(&b["rotation"], [0.0; 3]);
    let mirror = b["mirror"].as_bool().unwrap_or(false);
    let mut polygons = Vec::new();
    for c in b["cubes"].as_array().into_iter().flatten() {
        cube(c, mirror, pivot, texture, &mut polygons);
    }
    let mut children = Vec::new();
    for (i, child) in bones.iter().enumerate() {
        if parent(child) == Some(index) {
            if seen.contains(&i) {
                return Err(format!("bone {} is its own ancestor", child["name"]));
            }
            seen.push(i);
            children.push(bone(i, bones, parent, pivot.to_array(), texture, seen)?);
        }
    }
    children.sort_by(|a, b| a.name.cmp(&b.name));
    let offset = pivot - glam::Vec3::from_array(parent_pivot);
    Ok(Part {
        name: b["name"].as_str().unwrap_or("").to_owned(),
        pose: [
            offset.x,
            offset.y,
            offset.z,
            r[0].to_radians(),
            r[1].to_radians(),
            r[2].to_radians(),
            1.0,
            1.0,
            1.0,
        ],
        polygons,
        children,
    })
}

/// One cube as `ModelPart.Cube` bakes it (box UV, or Bedrock's per-face UV), relative to its
/// bone's pivot, turned about its own pivot when it has a rotation.
fn cube(
    c: &Value,
    bone_mirror: bool,
    pivot: glam::Vec3,
    texture: [f32; 2],
    out: &mut Vec<Polygon>,
) {
    let origin = v3(&c["origin"], [0.0; 3]);
    let [w, h, d] = v3(&c["size"], [0.0; 3]);
    let grow = c["inflate"].as_f64().unwrap_or(0.0) as f32;
    let mirror = c["mirror"].as_bool().unwrap_or(bone_mirror);
    // The box in vanilla's space: Bedrock's origin is its lowest corner.
    let (mut x0, y0, z0) = (
        origin[0] - grow,
        24.0 - origin[1] - h - grow,
        origin[2] - grow,
    );
    let (mut x1, y1, z1) = (
        origin[0] + w + grow,
        24.0 - origin[1] + grow,
        origin[2] + d + grow,
    );
    if mirror {
        std::mem::swap(&mut x0, &mut x1);
    }
    let corners = [
        [x0, y0, z0],
        [x1, y0, z0],
        [x1, y1, z0],
        [x0, y1, z0],
        [x0, y0, z1],
        [x1, y0, z1],
        [x1, y1, z1],
        [x0, y1, z1],
    ];
    // Box UV offsets (ModelPart.Cube): u, u+d, u+d+w, u+d+2w, u+2d+w, u+2d+2w; v, v+d, v+d+h.
    let at = |i: usize| c["uv"][i].as_f64().unwrap_or(0.0) as f32;
    let (u, v) = (at(0), at(1));
    let us = [
        u,
        u + d,
        u + d + w,
        u + d + w + w,
        u + d + w + d,
        u + d + w + d + w,
    ];
    let vs = [v, v + d, v + d + h];
    // Vanilla's faces in its order: corners, box UV (u0, v0, u1, v1), normal, and the face's
    // Bedrock name (vanilla's -y face is the top one).
    let faces: [([usize; 4], [f32; 4], [f32; 3], &str); 6] = [
        (
            [5, 4, 0, 1],
            [us[1], vs[0], us[2], vs[1]],
            [0.0, -1.0, 0.0],
            "up",
        ),
        (
            [2, 3, 7, 6],
            [us[2], vs[1], us[3], vs[0]],
            [0.0, 1.0, 0.0],
            "down",
        ),
        (
            [0, 4, 7, 3],
            [us[0], vs[1], us[1], vs[2]],
            [-1.0, 0.0, 0.0],
            "west",
        ),
        (
            [1, 0, 3, 2],
            [us[1], vs[1], us[2], vs[2]],
            [0.0, 0.0, -1.0],
            "north",
        ),
        (
            [5, 1, 2, 6],
            [us[2], vs[1], us[4], vs[2]],
            [1.0, 0.0, 0.0],
            "east",
        ),
        (
            [4, 5, 6, 7],
            [us[4], vs[1], us[5], vs[2]],
            [0.0, 0.0, 1.0],
            "south",
        ),
    ];
    let turn = c["rotation"].as_array().map(|_| {
        let center = [x0 + x1, y0 + y1, z0 + z1].map(|s| s / 2.0);
        let about = c["pivot"]
            .as_array()
            .map_or(glam::Vec3::from_array(center), |_| {
                point(v3(&c["pivot"], [0.0; 3]))
            });
        (rotation(v3(&c["rotation"], [0.0; 3])), about)
    });
    for (idx, box_uv, normal, name) in faces {
        let [u0, v0, u1, v1] = if c["uv"].is_object() {
            // Per-face UV: the face's own rectangle; a face not listed isn't drawn.
            let Some(face) = c["uv"].get(name) else {
                continue;
            };
            let at = |i: usize| face["uv"][i].as_f64().unwrap_or(0.0) as f32;
            let size = |i: usize| face["uv_size"][i].as_f64().unwrap_or(0.0) as f32;
            [at(0), at(1), at(0) + size(0), at(1) + size(1)]
        } else {
            box_uv
        };
        let uvs = [[u1, v0], [u0, v0], [u0, v1], [u1, v1]];
        let mut vertices: [[f32; 5]; 4] = std::array::from_fn(|i| {
            let mut p = glam::Vec3::from_array(corners[idx[i]]);
            if let Some((r, about)) = turn {
                p = r * (p - about) + about;
            }
            let p = p - pivot;
            [
                p.x,
                p.y,
                p.z,
                uvs[i][0] / texture[0],
                uvs[i][1] / texture[1],
            ]
        });
        let mut normal = glam::Vec3::from_array(normal);
        // ModelPart.Polygon: a mirrored face winds the other way and faces the other side.
        if mirror {
            vertices.reverse();
            normal.x = -normal.x;
        }
        if let Some((r, _)) = turn {
            normal = r * normal;
        }
        out.push(Polygon {
            normal: normal.to_array(),
            vertices,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn file(bones: Value) -> Value {
        json!({"format_version": "1.12.0", "minecraft:geometry": [{
            "description": {"identifier": "geometry.test", "texture_width": 64, "texture_height": 64},
            "bones": bones
        }]})
    }

    /// The zombie's head drawn in Blockbench bakes to vanilla's own polygons
    /// (`entity-models.json`, `minecraft:zombie#main`, head: `texOffs(0, 0).addBox(-4, -8, -4, 8, 8, 8)`).
    #[test]
    fn a_box_bakes_like_vanilla() {
        let model = parse(&file(json!([{"name": "head", "pivot": [0, 24, 0],
            "cubes": [{"origin": [-4, 24, -4], "size": [8, 8, 8], "uv": [0, 0]}]}])))
        .unwrap();
        assert_eq!(model.name, "root");
        let head = &model.children[0];
        assert_eq!((head.name.as_str(), head.pose), ("head", REST));
        let t = |u: f32, v: f32| [u / 64.0, v / 64.0];
        let vanilla = [
            (
                [0.0, -1.0, 0.0],
                [
                    [4., -8., 4., 16., 0.],
                    [-4., -8., 4., 8., 0.],
                    [-4., -8., -4., 8., 8.],
                    [4., -8., -4., 16., 8.],
                ],
            ),
            (
                [0.0, 1.0, 0.0],
                [
                    [4., 0., -4., 24., 8.],
                    [-4., 0., -4., 16., 8.],
                    [-4., 0., 4., 16., 0.],
                    [4., 0., 4., 24., 0.],
                ],
            ),
            (
                [-1.0, 0.0, 0.0],
                [
                    [-4., -8., -4., 8., 8.],
                    [-4., -8., 4., 0., 8.],
                    [-4., 0., 4., 0., 16.],
                    [-4., 0., -4., 8., 16.],
                ],
            ),
            (
                [0.0, 0.0, -1.0],
                [
                    [4., -8., -4., 16., 8.],
                    [-4., -8., -4., 8., 8.],
                    [-4., 0., -4., 8., 16.],
                    [4., 0., -4., 16., 16.],
                ],
            ),
            (
                [1.0, 0.0, 0.0],
                [
                    [4., -8., 4., 24., 8.],
                    [4., -8., -4., 16., 8.],
                    [4., 0., -4., 16., 16.],
                    [4., 0., 4., 24., 16.],
                ],
            ),
            (
                [0.0, 0.0, 1.0],
                [
                    [-4., -8., 4., 32., 8.],
                    [4., -8., 4., 24., 8.],
                    [4., 0., 4., 24., 16.],
                    [-4., 0., 4., 32., 16.],
                ],
            ),
        ];
        assert_eq!(head.polygons.len(), 6);
        for (p, (normal, verts)) in head.polygons.iter().zip(vanilla) {
            assert_eq!(p.normal, normal);
            for (got, want) in p.vertices.iter().zip(verts) {
                let [u, v] = t(want[3], want[4]);
                assert_eq!(*got, [want[0], want[1], want[2], u, v]);
            }
        }
    }

    /// Bones nest by `parent` with poses relative to the parent's pivot; rotations keep their
    /// signs in radians; a mirrored cube swaps its sides.
    #[test]
    fn bones_nest_rotate_and_mirror() {
        let model = parse(&file(json!([
            {"name": "body", "pivot": [0, 12, 0]},
            {"name": "arm", "parent": "body", "pivot": [5, 22, 0], "rotation": [90, -45, 30],
             "cubes": [{"origin": [4, 12, -2], "size": [2, 10, 4], "uv": [40, 16], "mirror": true}]},
        ])))
        .unwrap();
        let body = &model.children[0];
        assert_eq!(&body.pose[..3], &[0.0, 12.0, 0.0]);
        let arm = &body.children[0];
        assert_eq!(&arm.pose[..3], &[5.0, -10.0, 0.0]);
        let r = [90f32, -45.0, 30.0].map(f32::to_radians);
        assert_eq!(&arm.pose[3..6], &r);
        // The mirrored -y face: its corners reversed, its normal unchanged (x is 0).
        let top = &arm.polygons[0];
        assert_eq!(top.normal, [0.0, -1.0, 0.0]);
        assert_eq!(&top.vertices[0][..3], &[-1.0, 0.0, -2.0]);
        // Box UV from (40, 16): the -y face spans u 44..46, v 16..20 (reversed when mirrored).
        let uv = |v: [f32; 5]| [v[3] * 64.0, v[4] * 64.0];
        assert_eq!(uv(top.vertices[3]), [46.0, 16.0]);
        assert_eq!(uv(top.vertices[0]), [46.0, 20.0]);
        // The face labelled west lies on +x once mirrored, facing +x.
        let west = &arm.polygons[2];
        assert_eq!(west.normal, [1.0, 0.0, 0.0]);
        assert!(west.vertices.iter().all(|v| v[0] == 1.0));
    }

    #[test]
    fn per_face_uv_and_cube_rotation() {
        let model = parse(&file(json!([{"name": "fin", "pivot": [0, 0, 0], "cubes": [{
            "origin": [0, 0, 0], "size": [0, 4, 4], "pivot": [0, 0, 0], "rotation": [0, 90, 0],
            "uv": {"east": {"uv": [8, 8], "uv_size": [4, 4]}, "west": {"uv": [12, 8], "uv_size": [-4, 4]}}
        }]}])))
        .unwrap();
        let fin = &model.children[0];
        assert_eq!(fin.polygons.len(), 2);
        // The east face (+x) turned 90° about y faces -z.
        let east = &fin.polygons[1];
        let n = glam::Vec3::from_array(east.normal);
        assert!((n - glam::Vec3::new(0.0, 0.0, -1.0)).length() < 1e-6, "{n}");
        assert_eq!(
            [east.vertices[1][3], east.vertices[1][4]],
            [8.0 / 64.0, 8.0 / 64.0]
        );
    }

    #[test]
    fn legacy_format_and_errors() {
        let legacy = json!({"format_version": "1.8.0", "geometry.old": {
            "texturewidth": 32, "textureheight": 32,
            "bones": [{"name": "root", "pivot": [0, 0, 0],
                "cubes": [{"origin": [0, 0, 0], "size": [1, 1, 1], "uv": [0, 0]}]}]}});
        let model = parse(&legacy).unwrap();
        assert_eq!(
            (model.name.as_str(), model.children[0].name.as_str()),
            ("", "root")
        );
        assert_eq!(model.children[0].polygons[0].vertices[1][3], 1.0 / 32.0);
        assert!(parse(&json!({})).is_err());
        assert!(parse(&file(json!([{"name": "a"}, {"name": "a"}]))).is_err());
        assert!(
            parse(&file(
                json!([{"name": "a", "parent": "b"}, {"name": "b", "parent": "a"}])
            ))
            .is_err()
        );
    }
}
