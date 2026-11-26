# Ice and Fire on Horizon: the plan for its AI

What it takes to port Ice and Fire's creatures and their behaviour to Horizon, and run them on
the desktop and in the iOS app. Written for whoever continues, person or agent; read
[HANDOFF.md](HANDOFF.md) first. Nothing here is built yet: this is the plan.

## Where things stand (2026-10-31)

- **Models and animations run.** `horizon-port-pack` converts the mod's Tabula models
  (`crates/horizon-port-pack/src/tabula.rs`): all 192 of Ice and Fire 2.0's `.tbl`, and every
  pose folder into animations (`firedragon_walk` from `walk1`..`walk4`, `firedragon_flight`,
  `_bite`, `_roar`, `_sleeping`, ...). A fire dragon declared from them (zombie template,
  `red_4` texture) renders, walks, idles, bites and dies on the desktop **and on the iPhone**
  (player's `latest.log`, 12:37: "Summoned new Fire Dragon").
- **Server mods can steer it** (capability `entities`, docs/mods.md): walk-to, look-at, push
  and gravity (flight), play any animation (synced to players), hurt-area (breath), target a
  mob or the nearest player, scale (growth), data kept in the save, spawn, and players' right
  clicks (`interactions`). The `mob-orders` example drives a dragon with `/heel`, `/animate`,
  `/breathe`, `/hover`, `/grow`, `/attack`, `/raise`.
- **What runs the behaviour today is the template's** (a zombie's goals). Ice and Fire's own AI
  is not ported: that is this plan.

## The licence decides the shape

The jar is **Ice And Fire Community Edition 2.0** (`modId iceandfire`, authors IAFEnvoy, xiaowu,
uoay), `license = "LGPL-3.0"`, source at https://github.com/IAFEnvoy/IceAndFire-CE. Horizon is MIT.

- A port of its AI is a derivative of LGPL code: it **may** be written by following the Java
  source closely, but it must stay **LGPL-3.0**, credit the authors, say it is a modified,
  unofficial port, and keep the licence text. It must **not** go into Horizon's MIT tree.
- So the port is **its own project**, e.g. `~/Projects/iceandfire-horizon` (a separate git
  repository, LGPL-3.0, "Ice and Fire CE (unofficial Horizon port)"). Horizon only gains
  general APIs (MIT), never Ice and Fire code or art.
- **Check before sharing anything:** whether the CE repository's LGPL covers its textures,
  models and sounds too (look for a separate asset licence in the repo). Until that is
  confirmed, the port's built `.hmod`s stay on the player's machine, like today's test mod.
- Depends on IAFEnvoy's libraries `uranus` and `jupiter` (in the jar's `mods.toml`): their
  licences need the same check if any of their logic is ported.

## How it is built

Two mods, both made on the player's computer from the CE source (rules, numbers) and their
own jar (assets):

1. **Content mod `iceandfire`** (`.hmod`, no code): what `horizon-port-pack port` carries
   (blocks, items, recipes, loot, tags, lang, worldgen, textures, converted Tabula models and
   pose animations), plus **entity declarations** written by a generator script in the port
   repo (`gen_entities.py`): one per creature and variant, template, size, attributes, model,
   texture, `animations` states.
2. **Server mod `iceandfire_ai`** (`.hmod` with WASM, `api = "0.3"`, capabilities `entities`,
   `players`, `world`, `storage`): the behaviour, in Rust against `horizon-mod-sdk`'s
   `ServerMod`, ported from the Java classes below. It runs where the world runs (the server
   thread in single player, a LAN host, a dedicated server) and on iOS through Pulley.

Why a WASM server mod and not a native mixin mod (`.hnmod`): it runs on iOS, it is sandboxed,
it reloads live with `--dev-mod`, and it keeps the LGPL code out of Horizon's binary.

### What to read in the CE source (version matching jar 2.0)

| Area | Classes (package `com.iafenvoy.iceandfire`) |
|---|---|
| Dragon state, growth, eggs, taming, riding | `entity/DragonBaseEntity`, `FireDragonEntity`, `IceDragonEntity`, `LightningDragonEntity`, `entity/util/dragon/DragonSize`, `DragonUtils` |
| The behaviour loop | `entity/util/dragon/IafDragonLogic` (server and client ticks) |
| Flight | `entity/util/dragon/IafDragonFlightManager`, `ai/FlyingAITargetGoal` |
| Attacks and breath | `entity/util/dragon/IafDragonAttacks`, `IafDragonDestructionManager`, `entity/*DragonChargeEntity` |
| Goals | `ai/DragonAI*Goal` (AttackMelee, Escort, LookIdle, Mate, ReturnToRoost, Target, TargetItems, TargetNonTamed, Wander, WatchClosest), `ai/IAFLookControl` |
| Hitboxes | `entity/DragonPartEntity`, `MultipartPartEntity` |
| Client animation (what plays when) | `render/model/...` and the animator that blends poses |
| Config numbers | `config/` (breath damage, griefing, ages, spawn chances) |

Scope: 56 entity classes and 68 AI goals in total, 1.3 MB of entity bytecode. **Dragons
first**; the rest (hippogryph, sea serpent, cyclops, gorgon, pixie, troll, dread mobs,
death worm, amphithere, stymphalian bird, hydra, siren, ghost) after, reusing the same API.

## What Horizon still needs (MIT API work), in the order the milestones need it

Each is a small, general addition to capability `entities` (or a sibling), useful to any mod,
with tests, docs (docs/mods.md) and an `api_level` entry:

1. **Per-mob texture** (order `texture(string)`, synced like animations): dragon colours
   (red, green, bronze, gray, blue, white, sapphire, silver, electric, amethyst, copper,
   black) and stages, eyes overlays, sleeping eyes. Today a texture is per type.
2. **Sounds and particles** (orders `sound(name, volume, pitch)`, `particles(name, at, delta,
   count)`): roars, breath fire and frost, wing flaps; vanilla's level packets do the rest.
3. **Animation blending on the client** (mobs/geo.rs): cross-fade between animations over a
   few ticks, and layers (flight loop plus breath plus head look), as the mod's animator blends
   poses. Without it, state changes snap.
4. **Read game rules and difficulty** (`world.game-rule(name)`): `mobGriefing` gates breath
   destruction.
5. **Items in interactions** (`players.take-item(player, item, count)`, `give-item`) and the
   **sneaking flag** in `interaction`: feeding, taming, the dragon staff, sitting.
6. **Target a player by UUID** (today: `nearest-player`). Needs the server's mob→player binding
   (`LanHost::mob_players`) to accept an order's choice.
7. **Riding with player control** (the large one): an order to seat a player, and the rider's
   movement input (forward, strafe, jump, sneak, look) in `mobs()` so the mod flies the dragon;
   the client's camera and dismount. Builds on the game's existing riding code
   (`game/mounts.rs`, `mobs/riders.rs`).
8. **Projectiles** (order `shoot(kind, at, velocity, power)`): blast charges, a fireball-like
   entity that explodes or freezes where it lands.
9. **Multipart hitboxes** (later): dragons are hit on their head, neck, wings, tail. A first
   version uses one big box (declared `width` / `height` times `scale`).
10. **Spawning in the world** (roadmap's "Spawning and worldgen"): a hook when chunks generate
    or load, so the mod places roosts' and caves' dragons; spawn rules per biome.
11. **Piglin-style brains** still override orders (mobs/ai.rs `species_ai_step`): only matters
    if a creature uses a piglin or hoglin template.

## Milestones (each ends with an in-game check, desktop then iPhone)

- **M0. Set up.** Clone IceAndFire-CE at the 2.0 tag next to Horizon. Make the port repo
  (LGPL-3.0, README with credits and "unofficial, modified"). Run `horizon-port-pack port` on
  the player's jar into it, write `gen_entities.py`, and get all three dragons (fire, ice,
  lightning) declared and summonable. Settle the template: a zombie walks and melees; flight is
  the mod's.
- **M1. A dragon on the ground.** Port `IafDragonLogic`'s ground half and the Dragon*Goals
  into `iceandfire_ai`: wander, look idle, watch closest, target non-tamed, melee bite (the
  `bite1-3` animations), roar, sleep at night (`sleeping` held), return to roost, growth by
  age (data `age`, `stage`; `scale` per `DragonSize`), death. Needs API 1 (colours) and 2
  (sounds) to look and sound right.
- **M2. Breath.** Stream and blast breath from `IafDragonAttacks`: `hurt-area` cones along the
  head's look (a few balls per tick), fire or ice set on blocks and destruction through
  `world.set-blocks`, behind `mobGriefing` (API 4), particles (API 2), charge animations
  (`attack_stream_charge1-3`).
- **M3. Flight.** `IafDragonFlightManager` as a state machine in the mod: take off, hover, fly
  to targets and around, dive, land; `push` and `gravity` each tick; `flight`, `hovering`,
  `diving` animations with cross-fade (API 3).
- **M4. Eggs and taming.** Egg blocks and items from the content mod; hatching where the mod
  finds eggs (polling `world.get-block` near players until API 10); feeding and taming with
  `interactions` and item use (API 5); owner kept in data; follow, sit, escort; the dragon
  staff.
- **M5. Riding.** API 7, then the mod's rider controls (ground and flight), the player's
  attack and breath keys.
- **M6. The world.** Roosts, caves, dragon skeletons and their spawns (API 10).
- **M7. The other creatures,** one by one, smallest first (pixie, hippogryph, cyclops, sea
  serpent...).

## Performance to watch

- **Resource loading on iOS went from 12.7 s to 20.6 s** with the one content mod on (player's
  `latest.log`: "content from 1 mod(s) ... resources loaded in 20.6s"). Find out whether the
  content root (`mod_content::resource_root`, `.horizon-cache/mod-content`) is rebuilt every
  start instead of reused, and what the 192-model port will cost. Measure before M0's full port.
- The server mod runs on **Pulley** (an interpreter) on iOS: budget per tick (`FUEL_TICK`,
  50 M instructions), so think a few dragons at a time, steer each every 2 to 4 ticks, and keep
  the breath maths cheap. StikDebug's JIT (HANDOFF.md, "Faster mods with a JIT") would lift it.
- Each order is a host call: batch a tick's orders into one `give-orders`.

## How to work on it

- The dev loop from this session: build the game with `cargo build --release -p
  horizon-client --bins --features mods`; launch in scratch folders (CLAUDE.md "Keep your runs
  away from real player data"; not a scratch `XDG_RUNTIME_DIR`); `--dev-mod` the content mod
  folder and the server mod `.hmod` (it reloads when rebuilt); drive server mods with
  `horizon-ctl chat "/cmd"` (`horizon-ctl command` never reaches them); `horizon-ctl
  screenshot` and crop to check poses.
- The converted fire dragon test mod (`iaftest`) was made by hand in a scratch folder and is
  gone with that session: M0's generator replaces it. Its recipe: `firedragon_ground.geo.json`
  and its animation file from the carried `assets/iceandfire/{geo,animations}/tabula/firedragon/`,
  texture `textures/entity/firedragon/red_4.png`, template `minecraft:zombie`, animations idle
  `animation.firedragon_tail`, walk `animation.firedragon_walk`, attack `animation.firedragon_bite`.
- Mod loot tables use 26.3's format (`modifier` and `type`, not `functions` and `function`).
- Ship to the phone: push to the branch, CI's `ios` workflow builds `Horizon.ipa` (about 10
  minutes); the mods go in `Documents/horizon-mods/`, are turned on in the launcher's Mods page,
  and content mods take effect at the next start.
