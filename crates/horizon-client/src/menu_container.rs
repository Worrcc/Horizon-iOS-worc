//! Read-only menu rules for container integrations.
use super::*;

impl Menu {
    /// Component-aware item stack limit shared with client integrations.
    pub fn item_max_stack(items: &crate::items::Items, stack: &ItemStack) -> u32 {
        stack
            .data()
            .and_then(|d| {
                if d.other("!minecraft:max_stack_size").is_some() {
                    return None;
                }
                match d.other("minecraft:max_stack_size") {
                    Some(horizon_world::nbt::Tag::Int(n)) if (1..=99).contains(n) => {
                        Some(*n as u32)
                    }
                    _ => None,
                }
            })
            .unwrap_or_else(|| items.get(&stack.item).map_or(64, |i| i.max_stack))
    }

    /// Read-only slot placement rules for client container integrations. Uses
    /// the same item components, native overrides and slot caps as click().
    pub fn inspect_slot(
        &self,
        env: &Env,
        index: usize,
        item: &ItemStack,
    ) -> Option<(bool, u32, bool)> {
        let role = self.slots.get(index)?.role;
        let result = matches!(
            role,
            Role::CraftResult
                | Role::Output
                | Role::MerchantResult
                | Role::LoomResult
                | Role::StoneResult
                | Role::GrindResult
                | Role::CombineResult
                | Role::CartoResult
                | Role::ModResult
        );
        Some((
            self.may_place(env, index, item),
            self.slot_max(env, index, item),
            result,
        ))
    }
}
