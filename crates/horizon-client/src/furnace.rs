//! `AbstractFurnaceBlockEntity`'s cooking over the furnace data the chunk
//! holds (`containers::Furnace`): it needs the recipes, so it lives above them.
use crate::{
    containers::{Furnace, recipe_used_key},
    crafting::{Cooking, Recipes},
    items::{ItemStack, Items},
    providers::Providers,
};

/// What a stretch of unloaded time did to a furnace: the LIT block state it must
/// end on (`None` when it did not change), and the items it let fall.
#[derive(Debug, Default, PartialEq)]
pub struct CatchUpReport {
    pub lit: Option<bool>,
    pub drops: Vec<ItemStack>,
}

/// The furnace methods that read the recipes.
pub trait FurnaceCooking {
    /// `AbstractFurnaceBlockEntity.setItem` on the input slot: anything but
    /// the same item with the same components restarts cooking, with the
    /// total time of the new input's recipe (200 ticks without one).
    fn set_input(&mut self, recipes: &Recipes, stack: Option<ItemStack>);
    /// `recipesUsed`, each recipe with its `experience` (0 for one this data pack does
    /// not have), in the order `getRecipesToAwardAndPopExperience` pays them.
    fn recipe_experience(&self, recipes: &Recipes) -> Vec<(u32, f32)>;
    /// A menu click wrote its slots into `items`, and `before` was the input
    /// slot until then: the menu called `setItem` on it only if it changed
    /// (`Slot.setByPlayer`; partial takes and merges keep the stack).
    fn input_clicked(&mut self, recipes: &Recipes, before: Option<ItemStack>);
    /// One server tick. Returns `Some(lit)` when the LIT block state must change.
    /// `drops` collects what the furnace lets fall at its position
    /// (`Containers.dropItemStack`): the remainder of a fuel when more of it is left.
    fn tick(
        &mut self,
        recipes: &Recipes,
        items: &Items,
        providers: &Providers,
        drops: &mut Vec<ItemStack>,
    ) -> Option<bool>;
    /// `elapsed` ticks of [`tick`] in one call, as if the furnace had been ticked
    /// each of them. Only the steps that change nothing but a counter are skipped:
    /// the rest go through `tick` itself, so the two cannot drift apart.
    fn catch_up(
        &mut self,
        recipes: &Recipes,
        items: &Items,
        providers: &Providers,
        elapsed: u64,
    ) -> CatchUpReport;
    fn idle(&self) -> bool;
}
impl FurnaceCooking for Furnace {
    fn set_input(&mut self, recipes: &Recipes, stack: Option<ItemStack>) {
        let same = match (&self.items[0], &stack) {
            (Some(old), Some(new)) => old.same_item_same_components(new),
            _ => false,
        };
        self.items[0] = stack;
        if !same {
            self.cooking_total = self.items[0]
                .as_ref()
                .and_then(|input| recipes.cook(self.kind.recipe_type(), input))
                .map_or(200, |recipe| total_time(recipe.time, self.speed));
            self.cooking_timer = 0;
        }
    }
    fn recipe_experience(&self, recipes: &Recipes) -> Vec<(u32, f32)> {
        self.recipes_used
            .iter()
            .map(|(id, count)| (*count, recipes.cooking_experience(id)))
            .collect()
    }
    fn input_clicked(&mut self, recipes: &Recipes, before: Option<ItemStack>) {
        if self.items[0] != before {
            let after = std::mem::replace(&mut self.items[0], before);
            self.set_input(recipes, after);
        }
    }
    fn tick(
        &mut self,
        recipes: &Recipes,
        items: &Items,
        providers: &Providers,
        drops: &mut Vec<ItemStack>,
    ) -> Option<bool> {
        let was_lit = self.lit_remaining > 0;
        let mut lit = false;
        if self.lit_remaining > 0 {
            self.lit_remaining -= 1;
            lit = self.lit_remaining > 0;
        }
        let fast = self.kind.fast();
        let has_ingredient = self.items[0].is_some();
        let has_fuel = self.items[1].is_some();
        if lit || has_fuel && has_ingredient {
            if let Some(ingredient) = self.items[0].clone() {
                if let Some(recipe) = recipes.cook(self.kind.recipe_type(), &ingredient) {
                    if self.cooking_timer == 0 {
                        self.cooking_total = total_time(recipe.time, self.speed);
                    }
                    if can_burn(self, recipe, items) {
                        if !lit {
                            let fuel = self.items[1].clone();
                            let time = fuel
                                .as_ref()
                                .map_or(0, |f| providers.burn_time(items, f, fast));
                            let speed = fuel
                                .as_ref()
                                .map_or(1.0, |f| providers.speed(items, f, fast));
                            self.lit_remaining = time;
                            self.lit_total = time;
                            self.speed = speed;
                            if self.cooking_total > 0 && self.cooking_timer < self.cooking_total {
                                let ratio = self.cooking_timer as f32 / self.cooking_total as f32;
                                self.cooking_total = total_time(recipe.time, self.speed);
                                self.cooking_timer =
                                    (ratio * self.cooking_total as f32).ceil() as i32;
                            }
                            if time > 0 {
                                // AbstractFurnaceBlockEntity.consumeFuel: the fuel's crafting
                                // remainder takes the slot once the stack is used up, else
                                // it is dropped.
                                if let Some(f) = self.items[1].as_mut() {
                                    let remainder = items
                                        .get(&f.item)
                                        .and_then(|i| i.crafting_remainder.as_deref())
                                        .map(|r| ItemStack::new(r, 1));
                                    f.count -= 1;
                                    if f.count == 0 {
                                        self.items[1] = remainder;
                                    } else if let Some(remainder) = remainder {
                                        drops.push(remainder);
                                    }
                                }
                                lit = true;
                            }
                        }
                        if lit {
                            self.cooking_timer += 1;
                            if self.cooking_timer >= self.cooking_total {
                                self.cooking_timer = 0;
                                self.cooking_total = total_time(recipe.time, self.speed);
                                match &mut self.items[2] {
                                    Some(out) => out.count += recipe.result.count,
                                    None => self.items[2] = Some(recipe.result.clone()),
                                }
                                *self
                                    .recipes_used
                                    .entry(recipe_used_key(&recipe.id))
                                    .or_insert(0) += 1;
                                // AbstractFurnaceBlockEntity.burn: drying a wet sponge fills
                                // the bucket in the fuel slot.
                                if self.items[0]
                                    .as_ref()
                                    .is_some_and(|i| &*i.item == "minecraft:wet_sponge")
                                    && self.items[1]
                                        .as_ref()
                                        .is_some_and(|f| &*f.item == "minecraft:bucket")
                                {
                                    self.items[1] =
                                        Some(ItemStack::new("minecraft:water_bucket", 1));
                                }
                                if let Some(i) = self.items[0].as_mut() {
                                    i.count -= 1;
                                    if i.count == 0 {
                                        self.items[0] = None;
                                    }
                                }
                            }
                        } else {
                            self.cooking_timer = 0;
                        }
                    } else {
                        self.cooking_timer = 0;
                    }
                }
            } else {
                self.cooking_timer = 0;
            }
        } else if self.cooking_timer > 0 {
            self.cooking_timer = (self.cooking_timer - 2).clamp(0, self.cooking_total);
        }
        (was_lit != lit).then_some(lit)
    }
    fn catch_up(
        &mut self,
        recipes: &Recipes,
        items: &Items,
        providers: &Providers,
        elapsed: u64,
    ) -> CatchUpReport {
        let was_lit = self.lit_remaining > 0;
        let mut report = CatchUpReport::default();
        let mut left = elapsed;
        while left > 0 && !self.idle() {
            let before = self.clone();
            let lit = self.lit_remaining > 0;
            let cookable =
                cook_recipe(self, recipes).is_some_and(|recipe| can_burn(self, recipe, items));
            // Lit and cooking: each tick is `timer += 1` and `lit_remaining -= 1` until
            // the item finishes or the fuel runs out, so jump to the nearer of those.
            if lit && cookable && self.cooking_timer >= 1 && self.cooking_timer < self.cooking_total {
                let steps = left
                    .min((self.lit_remaining - 1) as u64)
                    .min((self.cooking_total - 1 - self.cooking_timer) as u64);
                if steps > 0 {
                    self.cooking_timer += steps as i32;
                    self.lit_remaining -= steps as i32;
                    left -= steps;
                    continue;
                }
            }
            // Not lit, and no fuel-and-ingredient pair: only the idle decay runs, which
            // is `timer = clamp(timer - 2, 0, total)` every tick, so it closes to a formula.
            if !lit
                && !(self.items[0].is_some() && self.items[1].is_some())
                && self.cooking_timer > 0
            {
                let first = (self.cooking_timer - 2).clamp(0, self.cooking_total) as i64;
                let steps = i64::try_from(left).unwrap_or(i64::MAX);
                self.cooking_timer = first
                    .saturating_sub(2i64.saturating_mul(steps - 1))
                    .max(0) as i32;
                break;
            }
            // Lit, but nothing cooks (no ingredient, no recipe, or the output is full):
            // after one tick the timer stays put, so the lit counter alone runs down.
            let steady = lit && !cookable;
            let mut drops = Vec::new();
            self.tick(recipes, items, providers, &mut drops);
            report.drops.extend(drops);
            left -= 1;
            // The first tick with a timer of 0 also resets the total to the recipe's,
            // so only bulk once that total is in place.
            let total_settled = cook_recipe(self, recipes)
                .is_none_or(|recipe| self.cooking_total == total_time(recipe.time, self.speed));
            if steady && total_settled && self.lit_remaining > 0 {
                let steps = left.min((self.lit_remaining - 1) as u64);
                self.lit_remaining -= steps as i32;
                left -= steps;
            }
            // A tick that changes nothing leaves the furnace that way for good.
            if *self == before {
                break;
            }
        }
        let now = self.lit_remaining > 0;
        report.lit = (was_lit != now).then_some(now);
        report
    }
    fn idle(&self) -> bool {
        self.lit_remaining <= 0
            && self.cooking_timer <= 0
            && !(self.items[0].is_some() && self.items[1].is_some())
    }
}
/// The recipe the input slot cooks, if any.
fn cook_recipe<'a>(furnace: &Furnace, recipes: &'a Recipes) -> Option<&'a Cooking> {
    let input = furnace.items[0].as_ref()?;
    recipes.cook(furnace.kind.recipe_type(), input)
}
/// `AbstractFurnaceBlockEntity.burn`'s output check: the result stacks onto the
/// output slot without passing the item's max stack.
fn can_burn(furnace: &Furnace, recipe: &Cooking, items: &Items) -> bool {
    let max = items.get(&recipe.result.item).map_or(64, |i| i.max_stack);
    match &furnace.items[2] {
        None => true,
        Some(out) => out.item == recipe.result.item && out.count + recipe.result.count <= max,
    }
}
fn total_time(time: u32, speed: f32) -> i32 {
    if speed > 0.0 {
        (time as f32 / speed).ceil() as i32
    } else {
        time as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{containers::FurnaceKind, items::Items};
    use horizon_world::loot::PackOverlay;
    use std::{collections::HashMap, fs};

    #[test]
    fn a_furnace_is_idle_only_while_its_tick_changes_nothing() {
        let stack = |id: &str| Some(ItemStack::new(id, 1));
        let furnace = |input, fuel, lit_remaining, cooking_timer| Furnace {
            kind: FurnaceKind::Furnace,
            items: [input, fuel, stack("minecraft:iron_ingot")],
            lit_remaining,
            lit_total: 0,
            cooking_timer,
            cooking_total: 200,
            speed: 1.0,
            recipes_used: HashMap::new(),
        };
        assert!(furnace(None, None, 0, 0).idle());
        assert!(furnace(stack("minecraft:iron_ore"), None, 0, 0).idle());
        assert!(furnace(None, stack("minecraft:coal"), 0, 0).idle());
        assert!(!furnace(stack("minecraft:iron_ore"), stack("minecraft:coal"), 0, 0).idle());
        assert!(!furnace(None, None, 1, 0).idle());
        assert!(!furnace(stack("minecraft:iron_ore"), None, 0, 5).idle());
    }

    /// A data root holding one recipe, `test:ore` into `test:ingot`, and an item table
    /// with two fuels: `test:coal` (no remainder) and `test:cell` (a content mod's item
    /// that leaves `test:husk`). The folder is gone when this returns.
    fn data(name: &str) -> (Items, Recipes, Providers) {
        let root =
            std::env::temp_dir().join(format!("horizon-furnace-{name}-{}", std::process::id(),));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("data/minecraft/recipe")).unwrap();
        fs::write(
            root.join("data/minecraft/recipe/test_ingot.json"),
            br#"{"type":"minecraft:smelting","ingredient":"test:ore","result":{"id":"test:ingot"}}"#,
        )
        .unwrap();
        fs::create_dir_all(root.join("data/minecraft/loot_table")).unwrap();
        let fuel_component =
            r#"{"minecraft:cooking_fuel": {"burn_time": 200, "speed_multiplier": 1.0}}"#;
        fs::write(
            root.join("items.json"),
            format!(
                r#"{{"test:ore": {{}}, "test:ingot": {{}}, "test:coal": {fuel_component}, "test:cell": {fuel_component}, "test:husk": {{}}}}"#
            ),
        )
        .unwrap();
        fs::write(
            root.join(horizon_items::items::MOD_CRAFT_REMAINDERS),
            r#"{"test:cell": "test:husk"}"#,
        )
        .unwrap();
        let items = Items::catalog(&root, &PackOverlay::NONE).unwrap();
        let recipes = Recipes::load(&root, &items, &PackOverlay::NONE).unwrap();
        let providers = Providers::load(&root);
        let _ = fs::remove_dir_all(&root);
        (items, recipes, providers)
    }

    /// The furnace sleeps while `idle`, so its tick must change nothing then: run the
    /// real tick on every idle case and compare, and check a furnace that can smelt
    /// isn't idle and does change.
    #[test]
    fn an_idle_furnace_tick_changes_nothing() {
        let (items, recipes, providers) = data("idle");
        let stack = |id: &str| Some(ItemStack::new(id, 1));
        let furnace = |input, fuel, output| Furnace {
            kind: FurnaceKind::Furnace,
            items: [input, fuel, output],
            lit_remaining: 0,
            lit_total: 0,
            cooking_timer: 0,
            cooking_total: 200,
            speed: 1.0,
            recipes_used: HashMap::new(),
        };
        let idle = [
            furnace(None, None, None),
            furnace(stack("test:ore"), None, None),
            furnace(None, stack("test:coal"), None),
            furnace(None, None, stack("test:ingot")),
            furnace(stack("test:ingot"), None, stack("test:ingot")),
        ];
        for before in idle {
            assert!(before.idle(), "{before:?}");
            let mut after = before.clone();
            let mut drops = Vec::new();
            let lit = after.tick(&recipes, &items, &providers, &mut drops);
            assert_eq!(lit, None, "{before:?}");
            assert!(drops.is_empty(), "{before:?}");
            assert_eq!(format!("{after:?}"), format!("{before:?}"));
        }
        let mut busy = furnace(stack("test:ore"), stack("test:coal"), None);
        assert!(!busy.idle());
        let before = format!("{busy:?}");
        assert_eq!(
            busy.tick(&recipes, &items, &providers, &mut Vec::new()),
            Some(true)
        );
        assert_ne!(format!("{busy:?}"), before);
    }

    /// A furnace with `fuel` x `count` burning `test:ore` into `test:ingot` ([`data`]).
    fn burn(fuel: &str, count: u32) -> (Furnace, Vec<ItemStack>) {
        let (items, recipes, providers) =
            data(&format!("remainder-{}-{count}", fuel.replace(':', "-")));
        let mut furnace = Furnace {
            kind: FurnaceKind::Furnace,
            items: [
                Some(ItemStack::new("test:ore", 1)),
                Some(ItemStack::new(fuel, count)),
                None,
            ],
            lit_remaining: 0,
            lit_total: 0,
            cooking_timer: 0,
            cooking_total: 0,
            speed: 1.0,
            recipes_used: HashMap::new(),
        };
        let mut drops = Vec::new();
        let lit = furnace.tick(&recipes, &items, &providers, &mut drops);
        assert_eq!(lit, Some(true), "the fuel lit the furnace");
        (furnace, drops)
    }

    fn fuel_slot(furnace: &Furnace) -> Option<(&str, u32)> {
        furnace.items[1].as_ref().map(|s| (&*s.item, s.count))
    }

    /// `AbstractFurnaceBlockEntity.consumeFuel`: the last of a fuel with a remainder
    /// leaves it in the fuel slot.
    #[test]
    fn the_last_fuel_leaves_its_remainder_in_the_slot() {
        let (furnace, drops) = burn("test:cell", 1);
        assert_eq!(fuel_slot(&furnace), Some(("test:husk", 1)));
        assert!(drops.is_empty());
    }

    /// With more of the fuel left, the remainder is dropped at the furnace.
    #[test]
    fn a_fuel_still_in_the_stack_drops_its_remainder() {
        let (furnace, drops) = burn("test:cell", 3);
        assert_eq!(fuel_slot(&furnace), Some(("test:cell", 2)));
        assert_eq!(drops, vec![ItemStack::new("test:husk", 1)]);
    }

    /// A fuel without a remainder leaves nothing.
    #[test]
    fn a_fuel_without_a_remainder_leaves_nothing() {
        let (furnace, drops) = burn("test:coal", 1);
        assert_eq!(fuel_slot(&furnace), None);
        assert!(drops.is_empty());
        let (furnace, drops) = burn("test:coal", 2);
        assert_eq!(fuel_slot(&furnace), Some(("test:coal", 1)));
        assert!(drops.is_empty());
    }

    /// The reference: `steps` real ticks, with the lit flip and drops they reported.
    fn ticked(
        start: &Furnace,
        steps: u64,
        (items, recipes, providers): &(Items, Recipes, Providers),
    ) -> (Furnace, CatchUpReport) {
        let mut furnace = start.clone();
        let mut report = CatchUpReport::default();
        for _ in 0..steps {
            let mut drops = Vec::new();
            furnace.tick(recipes, items, providers, &mut drops);
            report.drops.extend(drops);
        }
        let was_lit = start.lit_remaining > 0;
        let now = furnace.lit_remaining > 0;
        report.lit = (was_lit != now).then_some(now);
        (furnace, report)
    }

    /// Checks `catch_up(steps)` against `steps` real ticks from `start`.
    fn assert_matches_ticks(start: &Furnace, steps: u64, world: &(Items, Recipes, Providers)) {
        let (items, recipes, providers) = world;
        let (expected, expected_report) = ticked(start, steps, world);
        let mut caught = start.clone();
        let report = caught.catch_up(recipes, items, providers, steps);
        assert_eq!(
            caught, expected,
            "{start:?} after {steps} ticks: catch_up gave {caught:?}"
        );
        assert_eq!(report, expected_report, "{start:?} after {steps} ticks");
    }

    /// A small deterministic generator, so the random cases repeat.
    struct Lcg(u64);
    impl Lcg {
        fn below(&mut self, n: u64) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (self.0 >> 33) % n
        }
    }

    fn random_furnace(rng: &mut Lcg) -> Furnace {
        let input = match rng.below(3) {
            0 => None,
            1 => Some(ItemStack::new("test:ore", 1 + rng.below(3) as u32)),
            // An item with no recipe: it stays in the slot and cooks nothing.
            _ => Some(ItemStack::new("test:ingot", 1)),
        };
        let fuel = match rng.below(3) {
            0 => None,
            1 => Some(ItemStack::new("test:coal", 1 + rng.below(3) as u32)),
            _ => Some(ItemStack::new("test:cell", 1 + rng.below(3) as u32)),
        };
        let output = match rng.below(4) {
            0 => None,
            1 => Some(ItemStack::new("test:ingot", 1 + rng.below(63) as u32)),
            2 => Some(ItemStack::new("test:ore", 1)),
            _ => Some(ItemStack::new("test:ingot", 64)),
        };
        let cooking_total = 1 + rng.below(200) as i32;
        Furnace {
            kind: FurnaceKind::Furnace,
            items: [input, fuel, output],
            lit_remaining: if rng.below(2) == 0 {
                0
            } else {
                1 + rng.below(120) as i32
            },
            lit_total: 0,
            cooking_timer: rng.below(cooking_total as u64) as i32,
            cooking_total,
            speed: if rng.below(2) == 0 { 1.0 } else { 2.0 },
            recipes_used: HashMap::new(),
        }
    }

    /// Random furnaces over random spans: `catch_up` must land on the same furnace,
    /// drops and LIT flip as ticking each tick.
    #[test]
    fn catch_up_matches_ticking_each_tick() {
        let world = data("differential");
        let mut rng = Lcg(7);
        for _ in 0..400 {
            let start = random_furnace(&mut rng);
            let steps = rng.below(900);
            assert_matches_ticks(&start, steps, &world);
        }
    }

    /// A span is applied once, whatever the unloads and loads between its pieces:
    /// catching up in two pieces gives what one catch-up of the whole span gives.
    #[test]
    fn catch_up_in_pieces_matches_one_span() {
        let world = data("pieces");
        let (items, recipes, providers) = &world;
        let mut rng = Lcg(11);
        for _ in 0..200 {
            let start = random_furnace(&mut rng);
            let (a, b) = (rng.below(700), rng.below(700));
            let mut pieces = start.clone();
            pieces.catch_up(recipes, items, providers, a);
            pieces.catch_up(recipes, items, providers, b);
            let mut whole = start.clone();
            whole.catch_up(recipes, items, providers, a + b);
            assert_eq!(pieces, whole, "{start:?} after {a} then {b} ticks");
        }
    }

    fn with(input: Option<ItemStack>, fuel: Option<ItemStack>, output: Option<ItemStack>) -> Furnace {
        Furnace {
            kind: FurnaceKind::Furnace,
            items: [input, fuel, output],
            lit_remaining: 0,
            lit_total: 0,
            cooking_timer: 0,
            cooking_total: 200,
            speed: 1.0,
            recipes_used: HashMap::new(),
        }
    }

    /// The fuel runs out partway through a smelt; the item stops with its progress.
    #[test]
    fn fuel_runs_out_partway_through_a_smelt() {
        let world = data("fuel-out");
        let start = with(
            Some(ItemStack::new("test:ore", 1)),
            Some(ItemStack::new("test:coal", 1)),
            None,
        );
        for steps in [1, 199, 200, 201, 250, 399, 400, 401, 1000] {
            assert_matches_ticks(&start, steps, &world);
        }
    }

    /// The input runs out, the output gets the result, and the furnace goes idle.
    #[test]
    fn input_runs_out_and_the_result_is_taken_out() {
        let world = data("input-out");
        let start = with(
            Some(ItemStack::new("test:ore", 2)),
            Some(ItemStack::new("test:coal", 4)),
            None,
        );
        for steps in [0, 1, 150, 400, 401, 800, 5000] {
            assert_matches_ticks(&start, steps, &world);
        }
        let mut caught = start.clone();
        let (items, recipes, providers) = &world;
        caught.catch_up(recipes, items, providers, 5000);
        assert_eq!(caught.items[0], None);
        assert_eq!(caught.items[2], Some(ItemStack::new("test:ingot", 2)));
    }

    /// The output is nearly full: the smelt stops at the max stack, as in the ticks.
    #[test]
    fn a_full_output_stops_the_smelt() {
        let world = data("output-full");
        let start = Furnace {
            lit_remaining: 500,
            ..with(
                Some(ItemStack::new("test:ore", 3)),
                Some(ItemStack::new("test:coal", 2)),
                Some(ItemStack::new("test:ingot", 63)),
            )
        };
        for steps in [1, 200, 201, 600, 2000] {
            assert_matches_ticks(&start, steps, &world);
        }
    }

    /// Part of an item is cooked when the span ends, so the next catch-up resumes there.
    #[test]
    fn partial_progress_carries_over() {
        let world = data("partial");
        let start = Furnace {
            lit_remaining: 1000,
            ..with(
                Some(ItemStack::new("test:ore", 1)),
                Some(ItemStack::new("test:coal", 1)),
                None,
            )
        };
        for steps in [0, 1, 73, 199] {
            assert_matches_ticks(&start, steps, &world);
        }
        let (items, recipes, providers) = &world;
        let mut two_parts = start.clone();
        two_parts.catch_up(recipes, items, providers, 73);
        two_parts.catch_up(recipes, items, providers, 126);
        let mut one_part = start.clone();
        one_part.catch_up(recipes, items, providers, 199);
        assert_eq!(two_parts, one_part);
    }

    /// A cooling furnace with no fuel and ingredient loses its progress at the
    /// decay rate, and a span longer than the progress empties it.
    #[test]
    fn a_cooling_furnace_loses_progress_at_the_decay_rate() {
        let world = data("cooling");
        let start = Furnace {
            cooking_timer: 57,
            ..with(Some(ItemStack::new("test:ore", 1)), None, None)
        };
        for steps in [0, 1, 27, 28, 29, 100] {
            assert_matches_ticks(&start, steps, &world);
        }
    }

    /// A week of unloaded time finishes at once rather than ticking through it.
    #[test]
    fn a_very_long_span_is_finished_without_ticking_it() {
        let world = data("long");
        let (items, recipes, providers) = &world;
        let mut furnace = with(
            Some(ItemStack::new("test:ore", 64)),
            Some(ItemStack::new("test:coal", 64)),
            None,
        );
        let report = furnace.catch_up(recipes, items, providers, 1_000_000_000_000_000);
        assert_eq!(furnace.items[0], None);
        assert_eq!(furnace.items[1], None);
        assert_eq!(furnace.items[2], Some(ItemStack::new("test:ingot", 64)));
        assert_eq!(furnace.lit_remaining, 0);
        // It was unlit when the span began and is unlit when it ends.
        assert_eq!(report.lit, None);
    }
}
