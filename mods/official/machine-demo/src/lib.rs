use horizon_mod_sdk::{
    log::{self, Level},
    server::{
        Args, Argument, BehaviourConfig, BehaviourHand, BlockChangePos, BlockPos, Decision,
        MenuItem, Player, Registration, ServerMod, menus, players, server_hooks, world,
    },
};
#[derive(Default)]
struct Demo {
    due: std::collections::HashMap<(String, i32, i32, i32), u64>,
}
impl Demo {
    fn arm(&mut self, dimension: &str, pos: BlockPos) -> Result<(), String> {
        let now = world::time(dimension).game_time;
        self.due.retain(|_, due| *due >= now);
        let key = (dimension.to_owned(), pos.x, pos.y, pos.z);
        if self.due.get(&key).is_some_and(|due| *due >= now) {
            return Ok(());
        }
        world::schedule_tick(dimension, at(pos), 40)?;
        self.due.insert(key, now.saturating_add(40));
        Ok(())
    }
}
fn at(pos: BlockPos) -> BlockChangePos {
    BlockChangePos {
        x: pos.x,
        y: pos.y,
        z: pos.z,
    }
}
fn report(result: Result<(), String>) {
    if let Err(e) = result {
        log::write(Level::Warn, &e);
    }
}
impl ServerMod for Demo {
    fn init() -> Result<(Self, Registration), String> {
        server_hooks::set_behaviour_hooks(BehaviourConfig {
            block_interacted: true,
            block_scheduled_tick: true,
            ..Default::default()
        });
        server_hooks::watch_blocks(&["demo:smelter".into(), "minecraft:lectern".into()])?;
        Ok((
            Self::default(),
            Registration::new()
                .on_blocks()
                .command_with("demobreak", vec![Argument::block_pos("pos")]),
        ))
    }
    fn block_interacted(
        &mut self,
        p: &Player,
        pos: BlockPos,
        state: &str,
        _face: i32,
        _x: f64,
        _y: f64,
        _z: f64,
        hand: BehaviourHand,
        _item: &str,
        _sneaking: bool,
    ) -> Decision {
        if hand != BehaviourHand::Main {
            return Decision::Allow;
        }
        if state.split('[').next() == Some("minecraft:lectern") {
            let mut items = vec![None; 27];
            items[13] = Some(MenuItem {
                item: "minecraft:diamond_block".into(),
                count: 1,
                name: Some("menu.demo.diamond".into()),
                lore: vec!["menu.demo.diamond.lore".into()],
            });
            report(menus::open_menu(&p.id, 3, "menu.demo.machine", &items));
            return Decision::Cancel;
        }
        if state.split('[').next() == Some("demo:smelter") {
            report(self.arm(&p.dimension, pos));
        }
        Decision::Allow
    }
    fn menu_clicked(&mut self, p: &Player, slot: i32, button: i32, mode: i32) {
        log::write(
            Level::Info,
            &format!("machine-demo: click {} {slot} {button} {mode}", p.id),
        );
        if slot == 13 && button == 0 && mode == 0 {
            report(players::give_item(&p.id, "minecraft:diamond", 1));
        }
    }
    fn menu_closed(&mut self, p: &Player) {
        log::write(Level::Info, &format!("machine-demo: closed {}", p.id));
    }
    fn block_placed(&mut self, p: &Player, pos: BlockPos, state: &str) {
        if state.split('[').next() == Some("demo:smelter") {
            report(self.arm(&p.dimension, pos));
        }
    }
    fn block_scheduled_tick(&mut self, dim: &str, pos: BlockPos, _state: &str) {
        self.due.remove(&(dim.to_owned(), pos.x, pos.y, pos.z));
        report((|| {
            let moved = world::container_move(dim, at(pos), 0, at(pos), Some(1), 1)?;
            if moved > 0 {
                let next = world::block_data(dim, at(pos))?
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(0)
                    .saturating_add(1)
                    .to_string();
                world::set_block_data(dim, at(pos), Some(&next))?;
            }
            self.arm(dim, pos)
        })());
    }
    fn command(&mut self, p: &Player, _name: &str, args: &Args) -> Result<Option<String>, String> {
        let pos = args.block_pos("pos").ok_or("missing position")?;
        world::set_break_progress(&p.dimension, at(pos), 5)?;
        // Drops are spawned with all engine components intact.
        world::destroy_block(&p.dimension, at(pos), Some("minecraft:iron_pickaxe"), false)?;
        world::set_break_progress(&p.dimension, at(pos), -1)?;
        Ok(Some("machine-demo: broken".into()))
    }
}
horizon_mod_sdk::export_server_mod!(Demo);
