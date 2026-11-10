#!/usr/bin/env python3
"""API 20 run-mod-quiet through the integrated connection and a real LAN guest.

Build horizon-client with mods and horizon-ctl using the fast profile, then build/package
our SDK guests with `cargo test -p horizon-mods --features runtime,pack --test sandbox`.
Pass --resources to a prepared 26.3 directory; all game data goes in scratch folders.
"""
import argparse
import json
from pathlib import Path
import shutil

from lan_e2e import Game

ROOT = Path(__file__).resolve().parents[1]


def exercise(player, host, x):
    # The server command acts at the requesting player's feet, including for a guest
    # without access to vanilla commands. Its explicit broadcast is a hook side effect.
    host('command', command=f'tp {player.player} {x + 0.5} 100 20.5')
    player.wait(30)
    player.raw('chat', '/quiet_mod beacon 2')
    player.until(lambda: host.raw('block', x + 2, 101, 20),
                 lambda b: b['state'] == 'minecraft:glowstone', ticks=80)
    assert host.raw('block', x + 2, 100, 20)['state'] == 'minecraft:glowstone'
    assert host.raw('block', x + 2, 102, 20)['state'] == 'minecraft:air'
    before = player.raw('chat-log', 200)['chat']
    player.raw('chat', f'/quiet_mod ground;beacon invalid;no_such_mod_command;setblock {x + 2} 100 20 stone')
    player.wait(30)
    assert player.raw('chat-log', 200)['chat'] == before, 'quiet reply/error leaked to chat'
    assert host.raw('block', x + 2, 100, 20)['state'] == 'minecraft:glowstone', 'vanilla command ran'
    # The same success and argument error still reach chat for typed commands.
    player.raw('chat', '/ground')
    player.until(lambda: player.raw('chat-log', 200)['chat'],
                 lambda lines: any('standing on minecraft:stone' in str(line) for line in lines), ticks=80)
    before = player.raw('chat-log', 200)['chat']
    player.raw('chat', '/beacon invalid')
    player.until(lambda: player.raw('chat-log', 200)['chat'], lambda lines: lines != before, ticks=80)
    return {'pillar': [x + 2, 100, 20], 'quiet_feedback': 'none', 'typed_feedback': 'preserved'}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--resources', type=Path, required=True)
    parser.add_argument('--bin-dir', type=Path, default=ROOT / 'target/fast')
    parser.add_argument('--mods-dir', type=Path, default=ROOT / 'target/mods-test/mods')
    parser.add_argument('--out', type=Path, default=ROOT / '.local/mod-quiet-e2e')
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    mods = [args.mods_dir / name for name in ('beacons.hmod', 'horizon_mods_test_ui_guest.hmod')]
    for mod in mods:
        assert mod.is_file(), f'missing fixture: {mod}'
    games = [Game(name, (args.bin_dir / 'horizon-ctl').resolve(), args.resources.resolve())
             for name in ('host', 'guest')]
    host, guest = games
    # The launcher does not run here, but keep every launcher side-effect isolated too.
    for game in games:
        for key in ('HORIZON_LAUNCHER_TRASH', 'HORIZON_LAUNCHER_CLIPBOARD', 'HORIZON_LAUNCHER_DESKTOP'):
            game.d.env[key] = str(game.work / key.lower())
    launch = ['launch', '--offscreen', '--no-pause']
    for mod in mods:
        launch += ['--dev-mod', str(mod.resolve())]
    results = {}
    try:
        host.raw(*launch, '--name', 'QuietHost', '--game-dir', host.work)
        host.raw('open', 'QuietMods', 5, 'creative')
        host('wait_ready', seconds=400)
        host('command', command='gamerule spawn_mobs false')
        host('command', command='fill 10 99 10 50 99 30 minecraft:stone')
        host('command', command='fill 10 100 10 50 104 30 minecraft:air')
        host.player = 'QuietHost'
        results['integrated'] = exercise(host, host, 20)
        # No "commands" option: server-mod commands have the typed path's rights,
        # even for a guest who cannot run vanilla commands.
        port = host.raw('lan', 'open', 0)['port']
        guest.raw(*launch, '--game-dir', guest.work)
        guest.player = 'QuietGuest'
        guest.raw('connect', f'127.0.0.1:{port}', guest.player)
        guest.raw('wait-for', 'loaded', '--timeout', 300)
        guest.wait(40)
        results['guest'] = exercise(guest, host, 30)
        print(json.dumps(results, indent=2))
        (args.out / 'results.json').write_text(json.dumps(results, indent=2) + '\n')
    finally:
        for game in reversed(games):
            (args.out / f'{game.name}.log').write_text(game.close())
            shutil.rmtree(game.work, ignore_errors=True)


if __name__ == '__main__':
    main()
