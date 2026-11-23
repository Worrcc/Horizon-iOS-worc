use horizon_mod_sdk::server::{BlockChangePos, MenuItem, Player, menus, world};
pub fn run(p: &Player, mode: &str) -> Result<Option<String>, String> {
    let dim = &p.dimension;
    let pos = BlockChangePos { x: 0, y: 64, z: 0 };
    let icon = Some(MenuItem {
        item: "minecraft:stick".into(),
        count: 1,
        name: Some("icon".into()),
        lore: vec!["line".into()],
    });
    match mode {
        "menu-invalid" => {
            for rows in [0, 7, u32::MAX] {
                assert!(menus::open_menu(&p.id, rows, "title", &[]).is_err());
            }
            assert!(menus::open_menu("offline", 1, "title", &[]).is_err());
            assert!(menus::open_menu(&p.id, 1, &"é".repeat(129), &[]).is_err());
            assert!(menus::open_menu(&p.id, 1, "title", &vec![None; 10]).is_err());
            for bad in [
                MenuItem {
                    count: 0,
                    ..icon.clone().unwrap()
                },
                MenuItem {
                    count: 65,
                    ..icon.clone().unwrap()
                },
                MenuItem {
                    item: "missing:item".into(),
                    ..icon.clone().unwrap()
                },
                MenuItem {
                    lore: vec!["line".into(); 9],
                    ..icon.clone().unwrap()
                },
                MenuItem {
                    name: Some("x".repeat(257)),
                    ..icon.clone().unwrap()
                },
            ] {
                assert!(menus::open_menu(&p.id, 1, "title", &[Some(bad)]).is_err());
            }
            assert!(menus::set_menu_items(&p.id, &[]).is_err());
            assert!(menus::close_menu(&p.id).is_err());
            assert_eq!(menus::inventory_slot(3, 27), Some(9));
            assert_eq!(menus::inventory_slot(3, 54), Some(0));
            assert_eq!(menus::inventory_slot(7, 64), None);
            assert_eq!(menus::inventory_slot(3, -999), None);
        }
        "menu" => {
            menus::open_menu("Alice", 3, "menu", &[icon.clone()])?;
            menus::set_menu_items(&p.id, &[None, icon])?;
            menus::close_menu(&p.id)?;
        }
        "menu-cap" => {
            for _ in 0..64 {
                menus::open_menu(&p.id, 1, "menu", &[])?;
            }
            assert!(menus::open_menu(&p.id, 1, "menu", &[]).is_err());
        }
        "data" => {
            assert_eq!(world::block_data(dim, pos)?.as_deref(), Some("counter"));
            world::set_block_data(dim, pos, Some("next"))?;
            world::set_block_data(dim, pos, None)?;
            assert_eq!(world::block_data(dim, pos)?.as_deref(), Some("counter"));
        }
        "data-invalid" => {
            assert!(world::set_block_data(dim, pos, Some(&"é".repeat(2049))).is_err());
            assert!(world::block_data("missing:dim", pos).is_err());
            assert!(world::block_data(dim, BlockChangePos { x: 100, ..pos }).is_err());
            assert!(world::set_block_data(dim, BlockChangePos { x: 100, ..pos }, None).is_err());
        }
        "data-cap" => {
            for _ in 0..64 {
                world::set_block_data(dim, pos, Some("next"))?;
            }
            assert!(world::set_block_data(dim, pos, None).is_err());
        }
        "destroy" => {
            let drops = world::destroy_block(dim, pos, Some("minecraft:stick"), true)?;
            assert_eq!(drops.len(), 1);
            assert_eq!(drops[0].item, "minecraft:stick");
            assert_eq!(drops[0].count, 3);
            assert!(world::destroy_block(dim, pos, None, false)?.is_empty());
        }
        "destroy-cap" => {
            for _ in 0..64 {
                world::destroy_block(dim, pos, None, true)?;
            }
            assert!(world::destroy_block(dim, pos, None, true).is_err());
        }
        "destroy-invalid" => {
            assert!(world::destroy_block("missing:dim", pos, None, true).is_err());
            assert!(
                world::destroy_block(dim, BlockChangePos { x: i32::MAX, ..pos }, None, true)
                    .is_err()
            );
            assert!(world::destroy_block(dim, pos, Some("missing:item"), true).is_err());
        }
        "progress" => {
            for stage in -1..=9 {
                world::set_break_progress(dim, pos, stage)?;
            }
            for stage in [-2, 10, i32::MAX] {
                assert!(world::set_break_progress(dim, pos, stage).is_err());
            }
            assert!(world::set_break_progress("missing:dim", pos, 1).is_err());
        }
        "progress-cap" => {
            for _ in 0..64 {
                world::set_break_progress(dim, pos, 1)?;
            }
            assert!(world::set_break_progress(dim, pos, 1).is_err());
        }
        _ => return Err("unknown machines30 mode".into()),
    }
    Ok(Some("machines checked".into()))
}
