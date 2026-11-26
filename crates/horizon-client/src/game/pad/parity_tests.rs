//! A controller action does what its keyboard key does, on a real `Game` without a window.
//! What needs a renderer (inventory slots, screenshots) is covered by tools/pad_parity_e2e.py.

use super::super::{Audio, Game, Options, Progress, Screen, Settings, load_resources};
use horizon_client::gamepad::{Button, PadState};
use horizon_client::held::Source;
use winit::keyboard::KeyCode;

/// A game at play with no world: input reaches the mappings, as in `Screen::Playing`.
fn playing(test: impl FnOnce(&mut Game)) {
    let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
    let settings = Settings::default();
    let loaded = load_resources(
        root.clone(),
        None,
        &root,
        &settings,
        std::time::Instant::now(),
        false,
        true,
        &Progress::new(),
        &mut |_, _| unreachable!("no menu handoff"),
    )
    .unwrap();
    let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
    options.resources = root.clone();
    options.execution_role = crate::game::execution_role::ExecutionRole::IntegratedServer;
    let mut game = Game::new(
        &loaded.resources,
        loaded.pack,
        loaded.gui.unwrap(),
        options,
        settings,
        Audio::silent(&root),
        None,
    )
    .unwrap();
    game.title = None;
    game.screen = Screen::Playing;
    test(&mut game);
}

/// Hold `button` on the pad, as a poll that saw it go down would.
fn pad_press(game: &mut Game, button: Button, action: &str) {
    let mut state = PadState::default();
    state.set(button, true);
    game.pad.edges.update(&state, 0.4);
    game.pad_action(action, true).unwrap();
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn keyboard_handoff_keeps_what_the_pad_holds() {
    playing(|game| {
        game.mapped_key(KeyCode::KeyW, true, false, KeyCode::KeyW, Source::Keyboard)
            .unwrap();
        pad_press(game, Button::South, "key.jump");
        assert!(game.keys.contains(&KeyCode::Space) && game.keys.contains(&KeyCode::KeyW));
        // Splitscreen: keyboard and mouse move to another player.
        game.release_keyboard_mouse();
        assert!(
            !game.keys.contains(&KeyCode::KeyW),
            "the keyboard's walk ends"
        );
        assert!(
            game.keys.contains(&KeyCode::Space),
            "the pad still holds jump; only the keyboard and mouse left"
        );
    });
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn pad_smooth_camera_binding_toggles_the_cinematic_camera() {
    playing(|game| {
        game.settings
            .controller
            .bind("key.smoothCamera", Some(Button::Select));
        assert!(!game.smooth_camera);
        pad_press(game, Button::Select, "key.smoothCamera");
        assert!(game.smooth_camera, "as its key does in play");
    });
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn chord_resolution_reaches_mappings_and_keyboard_handoff_without_member_singles() {
    use horizon_client::gamepad::Binding;
    playing(|game| {
        game.settings.controller.bind("key.inventory", None);
        game.settings.controller.bind_combination(
            "key.sprint",
            Some(Binding::Chord(Button::North, Button::South)),
        );
        game.pad_resolve(&[(Button::South, true)]);
        game.pad_resolve(&[(Button::South, false)]);
        assert!(
            game.keys.contains(&KeyCode::Space),
            "quick single tap waits for its simulation tick"
        );
        game.pad_resolve(&[(Button::North, true), (Button::South, true)]);
        assert!(
            game.pad
                .holds_action(&game.settings.controller, "key.sprint")
        );
        assert!(!game.pad.holds_action(&game.settings.controller, "key.jump"));
        assert!(game.keys.contains(&KeyCode::ControlLeft));
        assert!(!game.keys.contains(&KeyCode::Space));
        game.release_keyboard_mouse();
        assert!(game.keys.contains(&KeyCode::ControlLeft));
        game.ticks += 1;
        game.pad_resolve(&[(Button::North, false)]);
        assert!(!game.keys.contains(&KeyCode::ControlLeft));
        assert!(
            !game
                .pad
                .holds_action(&game.settings.controller, "key.sprint")
        );
        game.pad_resolve(&[(Button::South, false)]);
        assert!(!game.keys.contains(&KeyCode::Space));
    });
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn virtual_pad_atomic_chords_and_hold_then_press_reach_the_same_resolver() {
    use horizon_client::gamepad::Binding;
    playing(|game| {
        game.settings.controller.bind_combination(
            "key.jump",
            Some(Binding::Chord(Button::LeftTrigger, Button::South)),
        );
        game.settings.controller.bind("key.use", None);
        game.pad_command(&["pad", "lt+south", "down"]).unwrap();
        let edges = game.pad.edges.update(&game.pad.injected, 0.4);
        assert!(edges.contains(&(Button::LeftTrigger, true)));
        assert!(edges.contains(&(Button::South, true)));
        game.pad_resolve(&edges);
        assert!(game.keys.contains(&KeyCode::Space));
        game.ticks += 1;
        game.pad_command(&["pad", "release"]).unwrap();
        let edges = game.pad.edges.update(&game.pad.injected, 0.4);
        game.pad_resolve(&edges);
        assert!(!game.keys.contains(&KeyCode::Space));
        game.pad_command(&["pad", "lt", "down"]).unwrap();
        let edges = game.pad.edges.update(&game.pad.injected, 0.4);
        game.pad_resolve(&edges);
        game.pad_command(&["pad", "south", "down"]).unwrap();
        let edges = game.pad.edges.update(&game.pad.injected, 0.4);
        game.pad_resolve(&edges);
        assert!(game.keys.contains(&KeyCode::Space));
        assert!(game.pad_command(&["pad", "south+oops", "down"]).is_err());
        assert!(game.pad.injected.held(Button::LeftTrigger));
        assert!(game.pad.injected.held(Button::South));
    });
}
