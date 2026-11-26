//! API 35 limits and pure computations shared by the host and its game adapter.
pub const ENTITY_RADIUS: f32 = 64.0;
pub const ENTITY_MAX: usize = 256;
pub const ENTITY_CALLS: u8 = 4;

pub fn validate_entity_query(radius: f32, max: u32) -> Result<(), String> {
    if !radius.is_finite() || !(0.0..=ENTITY_RADIUS).contains(&radius) {
        return Err("radius must be finite and in 0..64".into());
    }
    if max > ENTITY_MAX as u32 {
        return Err("max must be in 0..256".into());
    }
    Ok(())
}

/// Keep only the nearest bounded observations from one loaded-entity scan.
pub fn nearest<T>(mut entries: Vec<(f64, T)>) -> Vec<T> {
    entries.retain(|(distance, _)| {
        distance.is_finite() && (0.0..=f64::from(ENTITY_RADIUS).powi(2)).contains(distance)
    });
    if entries.len() > ENTITY_MAX {
        entries.select_nth_unstable_by(ENTITY_MAX, |a, b| a.0.total_cmp(&b.0));
        entries.truncate(ENTITY_MAX);
    }
    entries.sort_by(|a, b| a.0.total_cmp(&b.0));
    entries.into_iter().map(|(_, entry)| entry).collect()
}

#[derive(Clone, Debug, Default)]
pub struct EntityCallBudget(u8);
impl EntityCallBudget {
    pub fn reset(&mut self) {
        self.0 = 0;
    }
    pub fn take(&mut self) -> Result<(), String> {
        if self.0 >= ENTITY_CALLS {
            return Err("entities-near permits 4 calls per frame".into());
        }
        self.0 += 1;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    Hostile,
    Passive,
    Neutral,
    Water,
    Ambient,
    Player,
    OwnTamed,
    OtherTamed,
    Other,
}

/// Ownership wins over the species category; neutrality is a species trait,
/// not a transient target/anger state. Missing owner on a tame is other-tamed.
pub fn category(group: &str, neutral: bool, tame: bool, own: bool, player: bool) -> Category {
    if player {
        return Category::Player;
    }
    if tame {
        return if own {
            Category::OwnTamed
        } else {
            Category::OtherTamed
        };
    }
    if neutral {
        return Category::Neutral;
    }
    match group {
        "monster" => Category::Hostile,
        "creature" => Category::Passive,
        "axolotls" | "underground_water_creature" | "water_creature" | "water_ambient" => {
            Category::Water
        }
        "ambient" => Category::Ambient,
        _ => Category::Other,
    }
}

/// Items already holds saturation *points*, as FoodProperties.saturation does.
pub fn food_value(
    component: Option<(u32, f32)>,
    always: bool,
) -> Result<Option<(u32, f32, bool)>, String> {
    component
        .map(|(nutrition, saturation)| {
            if !saturation.is_finite() || saturation < 0.0 {
                return Err("invalid food saturation in loaded item data".into());
            }
            Ok((nutrition, saturation, always))
        })
        .transpose()
}

/// Union of actual GUI sprite rectangles, including the frame's jitter and
/// regeneration offset. No assumed desktop/touch coordinates.
pub fn union_bounds(rects: impl IntoIterator<Item = [f32; 4]>) -> Result<Option<[f32; 4]>, String> {
    let mut bounds: Option<[f32; 4]> = None;
    for [x, y, w, h] in rects {
        if ![x, y, w, h, x + w, y + h].into_iter().all(f32::is_finite) || w <= 0.0 || h <= 0.0 {
            return Err("invalid HUD bar rectangle".into());
        }
        bounds = Some(match bounds {
            None => [x, y, w, h],
            Some([bx, by, bw, bh]) => {
                let (left, top) = (x.min(bx), y.min(by));
                [
                    left,
                    top,
                    (x + w).max(bx + bw) - left,
                    (y + h).max(by + bh) - top,
                ]
            }
        });
    }
    Ok(bounds)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn api35_entity_validation_and_frame_cap() {
        for radius in [-1.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 64.01] {
            assert!(validate_entity_query(radius, 1).is_err());
        }
        assert!(validate_entity_query(64.0, 257).is_err());
        assert!(validate_entity_query(64.0, 256).is_ok());
        assert!(validate_entity_query(0.0, 0).is_ok());
        let mut budget = EntityCallBudget::default();
        for _ in 0..4 {
            assert!(budget.take().is_ok());
        }
        assert!(budget.take().is_err());
        budget.reset();
        assert!(budget.take().is_ok());
    }
    #[test]
    fn api35_nearest_caps_after_distance_selection() {
        let entries = (0..1000)
            .rev()
            .map(|i| ((i as f64 / 100.0).powi(2), i))
            .collect();
        assert_eq!(nearest(entries), (0..256).collect::<Vec<_>>());
        assert_eq!(
            nearest(vec![
                (4096.0, 1),
                (4096.01, 2),
                (f64::NAN, 3),
                (-1.0, 4),
                (0.0, 5)
            ]),
            [5, 1]
        );
        assert!(nearest::<u32>(vec![]).is_empty());
    }
    #[test]
    fn api35_categories_and_ownership() {
        for (group, expected) in [
            ("monster", Category::Hostile),
            ("creature", Category::Passive),
            ("ambient", Category::Ambient),
            ("water_creature", Category::Water),
            ("water_ambient", Category::Water),
            ("axolotls", Category::Water),
            ("underground_water_creature", Category::Water),
            ("misc", Category::Other),
        ] {
            assert_eq!(category(group, false, false, false, false), expected);
        }
        assert_eq!(
            category("monster", true, false, false, false),
            Category::Neutral
        );
        assert_eq!(
            category("creature", true, true, true, false),
            Category::OwnTamed
        );
        assert_eq!(
            category("creature", true, true, false, false),
            Category::OtherTamed
        );
        assert_eq!(
            category("misc", false, false, false, true),
            Category::Player
        );
    }
    #[test]
    fn api35_food_component_points_and_nonfood() {
        assert_eq!(
            food_value(Some((4, 2.4)), false).unwrap(),
            Some((4, 2.4, false))
        );
        assert_eq!(
            food_value(Some((4, 9.6)), true).unwrap(),
            Some((4, 9.6, true))
        );
        assert_eq!(food_value(None, false).unwrap(), None);
        for bad in [-1.0, f32::NAN, f32::INFINITY] {
            assert!(food_value(Some((1, bad)), false).is_err());
        }
    }
    #[test]
    fn api35_hud_bounds_follow_rows_mounts_and_touch_positions() {
        assert_eq!(
            union_bounds((0..10).map(|n| [69.0 + n as f32 * 8.0, 201.0, 9.0, 9.0])).unwrap(),
            Some([69.0, 201.0, 81.0, 9.0])
        );
        assert_eq!(
            union_bounds([
                [169.0, 201.0, 81.0, 9.0],
                [169.0, 191.0, 81.0, 9.0],
                [169.0, 181.0, 33.0, 9.0]
            ])
            .unwrap(),
            Some([169.0, 181.0, 81.0, 29.0])
        );
        assert_eq!(
            union_bounds([[14.0, 20.0, 9.0, 9.0], [22.0, 19.0, 9.0, 9.0]]).unwrap(),
            Some([14.0, 19.0, 17.0, 10.0])
        );
        assert_eq!(union_bounds([]).unwrap(), None);
        assert!(union_bounds([[f32::NAN, 0.0, 9.0, 9.0]]).is_err());
    }
}
