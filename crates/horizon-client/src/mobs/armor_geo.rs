//! Equipment geometry shares the creature baker, but attaches each vanilla bone to
//! the wearer's actual part matrix. Children retain their local bind poses.
use super::{Part, anim::Poses, render};
use crate::mesher::Vertex;
use std::collections::HashMap;

pub const SLOTS: [&str; 4] = ["helmet", "chestplate", "leggings", "boots"];
const PARTS: [&str; 6] = [
    "head",
    "body",
    "right_arm",
    "left_arm",
    "right_leg",
    "left_leg",
];
const PREFIX: &str = "horizon:armor/";

/// Select entire bone trees, never just the base cubes. A leggings file may
/// explicitly supply a body; an absent body does not create a vanilla one.
pub fn bake(file: &serde_json::Value) -> Result<HashMap<&'static str, Part>, String> {
    let model = super::geo::parse(file)?;
    fn check(p: &Part, anchored: bool) -> Result<(), String> {
        let vanilla = PARTS.contains(&p.name.as_str());
        if vanilla == anchored {
            return Err(format!(
                "armor bone {} must be a vanilla root or its child",
                p.name
            ));
        }
        for child in &p.children {
            check(child, true)?;
        }
        Ok(())
    }
    for bone in &model.children {
        check(bone, false)?;
    }
    if model.children.is_empty() {
        return Err("armor geometry has no vanilla bones".into());
    }
    Ok(SLOTS
        .into_iter()
        .map(|slot| {
            let mut part = model.clone();
            // The root name identifies this layer without changing the vanilla render API.
            part.name = format!("{PREFIX}{slot}");
            part.children.retain(|bone| match slot {
                "helmet" => bone.name == "head",
                "chestplate" => matches!(bone.name.as_str(), "body" | "right_arm" | "left_arm"),
                "leggings" => matches!(bone.name.as_str(), "body" | "right_leg" | "left_leg"),
                _ => matches!(bone.name.as_str(), "right_leg" | "left_leg"),
            });
            // A root's bind transform is replaced by the wearer's part transform.
            for bone in &mut part.children {
                bone.pose = [0., 0., 0., 0., 0., 0., 1., 1., 1.];
            }
            (slot, part)
        })
        .collect())
}

pub fn slot(part: &Part) -> Option<&str> {
    part.name.strip_prefix(PREFIX)
}

/// Attach to the complete part matrix, including ancestors' baby scales and
/// crouch offsets. Use separate poses so child names cannot collide with the wearer.
#[allow(clippy::too_many_arguments)]
pub fn draw(
    armor: &Part,
    wearer: &Part,
    base: glam::Mat4,
    poses: &Poses,
    uv: [f32; 4],
    color: u32,
    light: u32,
    overlay: u32,
    out: &mut Vec<Vertex>,
) {
    let local = Poses::new(armor);
    for bone in &armor.children {
        if let Some(matrix) = render::part_matrix(wearer, base, poses, &bone.name, 0.0) {
            render::draw(bone, matrix, &local, uv, color, light, overlay, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    const FIXTURE: &str =
        include_str!("../../tests/fixtures/armor-geo/assets/armor_fixture/geo/armor/horn.geo.json");

    #[test]
    fn armor_geo_slots_preserve_children_and_optional_leggings_body() {
        let mut file: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
        let slots = bake(&file).unwrap();
        assert_eq!(slots["helmet"].children[0].name, "head");
        assert_eq!(slots["helmet"].children[0].children[0].name, "horn");
        assert!(slots["leggings"].children.is_empty());
        let bones = file["minecraft:geometry"][0]["bones"]
            .as_array_mut()
            .unwrap();
        for name in &PARTS[1..] {
            bones.push(json!({"name":name,"pivot":[0,24,0]}));
        }
        let slots = bake(&file).unwrap();
        let names = |slot: &str| {
            slots[slot]
                .children
                .iter()
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>()
        };
        assert_eq!(names("chestplate"), ["body", "left_arm", "right_arm"]);
        assert_eq!(names("leggings"), ["body", "left_leg", "right_leg"]);
        assert_eq!(names("boots"), ["left_leg", "right_leg"]);
        bones_invalid(&mut file);
        assert!(bake(&file).is_err());
    }
    fn bones_invalid(file: &mut serde_json::Value) {
        file["minecraft:geometry"][0]["bones"][1]["parent"] = json!("missing");
    }

    #[test]
    fn armor_geo_draw_follows_wearer_matrix_and_keeps_horn_rotation_uv_and_glint() {
        let file = serde_json::from_str(FIXTURE).unwrap();
        let slots = bake(&file).unwrap();
        let armor = &slots["helmet"];
        let mut wearer = super::super::geo::parse(&file).unwrap();
        wearer.pose = [1., 3., 0., 0., 0., 0., 0.5, 0.5, 0.5];
        let head = &mut wearer.children[0];
        head.pose[0] = 2.;
        head.pose[1] = 4.;
        head.pose[4] = 0.7;
        let poses = Poses::new(&wearer);
        let base = glam::Mat4::from_rotation_x(0.2);
        let mut actual = Vec::new();
        draw(
            armor,
            &wearer,
            base,
            &poses,
            [0.1, 0.2, 0.5, 0.7],
            0xFFFF_FFFF,
            240,
            render::ARMOR_GLINT,
            &mut actual,
        );
        assert_eq!(actual.len(), 48); // Helmet and rotated horn, six quads each.
        // Independent model-space expectations: inflated helmet's first top corner
        // and the rotated horn's centre, then head rotation, offsets and baby scale.
        let expected = |[x, y, z]: [f32; 3]| {
            let (sin, cos) = 0.7_f32.sin_cos();
            let x1 = (1. + 0.5 * (2. + x * cos + z * sin)) / 16.;
            let y1 = (3. + 0.5 * (4. + y)) / 16.;
            let z1 = 0.5 * (z * cos - x * sin) / 16.;
            let (sin, cos) = 0.2_f32.sin_cos();
            glam::Vec3::new(x1, y1 * cos - z1 * sin, y1 * sin + z1 * cos)
        };
        assert!((glam::Vec3::from_array(actual[0].pos) - expected([5., -9., 5.])).length() < 1e-6);
        let centre = actual[24..]
            .iter()
            .map(|v| glam::Vec3::from_array(v.pos))
            .sum::<glam::Vec3>()
            / 24.;
        let (sin, cos) = 25_f32.to_radians().sin_cos();
        assert!((centre - expected([0., -8. - 3.5 * cos, 3.5 * sin])).length() < 1e-6);
        for v in &actual {
            assert_eq!(v.layer, render::ARMOR_GLINT);
            assert_eq!(v.color, 0xFFFF_FFFF);
            assert_eq!(v.light, 240);
            assert!((v.uv[0] - (0.1 + 0.4 * v.glint_uv[0])).abs() < 1e-6);
            assert!((v.uv[1] - (0.2 + 0.5 * v.glint_uv[1])).abs() < 1e-6);
        }
        let mut no_legs = Vec::new();
        draw(
            &slots["boots"],
            &wearer,
            base,
            &poses,
            [0., 0., 1., 1.],
            0,
            0,
            0,
            &mut no_legs,
        );
        assert!(no_legs.is_empty());
    }
}
