#!/usr/bin/env python3
"""Live check of API level 33 `atmosphere` with mods/official/atmosphere-demo in a scratch game.

Builds nothing: needs `target/fast/horizon-client` (feature `mods`), `target/fast/horizon-ctl` and
`target/horizon-mod/atmosphere_demo.hmod` (see the demo's README). Every run gets fresh homes.

    python3 tools/live_atmosphere.py [--fork DIR] [--resources DIR] [--keep]

Sets a distinctive fog colour (#ff00ff, fog full at 4 blocks, sky the same colour) and checks that the
middle of a screenshot turns magenta, then `/atmo off` and checks it fades back. Prints one line per
check and exits non-zero on a failed one.
"""
import argparse, json, os, struct, subprocess, sys, tempfile, time, zlib

ap = argparse.ArgumentParser()
here = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ap.add_argument("--fork", default=here)
local = os.path.join(here, ".local/horizon-client/26.3")
if not os.path.isdir(local):
    local = os.path.expanduser("~/Projects/Horizon-iOS-worc/.local/horizon-client/26.3")
ap.add_argument("--resources", default=local)
ap.add_argument("--keep", action="store_true", help="keep the scratch folder and screenshots")
args = ap.parse_args()
BIN = os.path.join(args.fork, "target/fast")
DEMO = os.path.join(args.fork, "target/horizon-mod/atmosphere_demo.hmod")
for need in (f"{BIN}/horizon-ctl", f"{BIN}/horizon-client", DEMO, args.resources):
    if not os.path.exists(need):
        sys.exit(f"missing {need}")

home = tempfile.mkdtemp(prefix="horizon-atmo-")
for d in ("data", "cfg", "lh", "game"):
    os.makedirs(f"{home}/{d}")
env = dict(os.environ, HORIZON_HOME=home, HORIZON_LAUNCHER_HOME=f"{home}/lh", XDG_DATA_HOME=f"{home}/data",
           XDG_CONFIG_HOME=f"{home}/cfg", HORIZON_LAUNCHER_NO_SELF_UPDATE="1", HORIZON_NO_AUDIO="1")
sock = f"{home}/ctl.sock"


def ctl(*a, timeout=60):
    r = subprocess.run([f"{BIN}/horizon-ctl", "--socket", sock, *map(str, a)], capture_output=True, text=True,
                       timeout=timeout, env=env, cwd=f"{home}/game")
    return r.stdout.strip() or r.stderr.strip()


def chat_lines():
    try:
        return [l.split("[CHAT] ", 1)[1].rstrip() for l in open(f"{home}/logs/latest.log", errors="replace") if "[CHAT] " in l]
    except OSError:
        return []


def say(line, wait=0.6):
    before = len(chat_lines())
    ctl("chat", line)
    time.sleep(wait)
    return chat_lines()[before:]


def png_pixels(path):
    """(width, height, rows of (r, g, b)) for an 8-bit RGB or RGBA PNG."""
    data = open(path, "rb").read()
    assert data[:8] == b"\x89PNG\r\n\x1a\n", "not a PNG"
    pos, idat, w = 8, b"", 0
    while pos < len(data):
        n, kind = struct.unpack(">I4s", data[pos:pos + 8])
        body = data[pos + 8:pos + 8 + n]
        if kind == b"IHDR":
            w, h, depth, color = struct.unpack(">IIBB", body[:10])
            assert depth == 8 and color in (2, 6), f"PNG depth {depth} colour type {color}"
            bpp = 3 if color == 2 else 4
        elif kind == b"IDAT":
            idat += body
        pos += 12 + n
    raw, stride, rows, prev = zlib.decompress(idat), w * bpp, [], bytearray(w * bpp)
    for y in range(h):
        f, line = raw[y * (stride + 1)], bytearray(raw[y * (stride + 1) + 1:(y + 1) * (stride + 1)])
        for i in range(stride):
            a = line[i - bpp] if i >= bpp else 0
            b, c = prev[i], prev[i - bpp] if i >= bpp else 0
            if f == 1: line[i] = (line[i] + a) & 255
            elif f == 2: line[i] = (line[i] + b) & 255
            elif f == 3: line[i] = (line[i] + (a + b) // 2) & 255
            elif f == 4:
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                line[i] = (line[i] + (a if pa <= pb and pa <= pc else b if pb <= pc else c)) & 255
        rows.append([tuple(line[x * bpp:x * bpp + 3]) for x in range(w)])
        prev = line
    return w, h, rows


def middle_colour(name):
    path = f"{home}/{name}.png"
    ctl("screenshot", path)
    for _ in range(40):
        if os.path.exists(path) and os.path.getsize(path) > 0:
            break
        time.sleep(0.25)
    time.sleep(0.3)
    w, h, rows = png_pixels(path)
    # The average of a box around the middle, under the crosshair's arms.
    box = [rows[y][x] for y in range(h // 2 - 40, h // 2 - 20) for x in range(w // 2 - 20, w // 2 + 20)]
    return tuple(sum(p[i] for p in box) // len(box) for i in range(3))


def magenta(c):
    r, g, b = c
    return r > 180 and b > 180 and g < 90


failed = []


def check(name, ok, detail):
    print(f"{'ok  ' if ok else 'FAIL'} {name}: {detail}")
    if not ok:
        failed.append(name)


print("launch:", ctl("launch", "--world", "atmo", "--creative", "--offscreen", "--wait", "--resources", args.resources,
                     "--game-dir", f"{home}/game", "--dev-mod", DEMO, timeout=240)[:60])
try:
    ctl("wait-ready", 90, timeout=120)
    ctl("command", "time set 6000"); ctl("command", "weather clear"); ctl("command", "gamerule doDaylightCycle false")
    ctl("look", 0, -5)
    time.sleep(1.0)
    out = say("/atmo off now", 3.0)
    vanilla = middle_colour("vanilla")
    check("vanilla is not magenta", not magenta(vanilla), f"middle {vanilla}")
    out = say("/atmo fog ff00ff 0 4", 1.5)
    check("demo defined the fog", any("ff00ff" in m for m in out), out[-2:])
    tinted = middle_colour("magenta")
    check("fog reads back magenta", magenta(tinted), f"middle {tinted}")
    out = say("/atmo conditions", 0.8)
    check("conditions read", any(" day " in m and "sky " in m for m in out), out[-1:])
    say("/atmo off", 3.5)
    back = middle_colour("back")
    check("off fades back to vanilla", not magenta(back) and sum(abs(a - b) for a, b in zip(back, vanilla)) < 60,
          f"middle {back} vs {vanilla}")
    logs = ctl("logs", "--lines", 400)
    bad = [l[-160:] for l in logs.splitlines() if "atmosphere" in l.lower() and ("warn" in l.lower() or "error" in l.lower())]
    check("no atmosphere warnings", not bad, bad[-3:])
finally:
    ctl("quit", timeout=30)
    for _ in range(40):
        if not os.path.exists(sock):
            break
        time.sleep(0.25)
    if args.keep:
        print("kept", home)
    else:
        subprocess.run(["rm", "-rf", home], stderr=subprocess.DEVNULL)
print("ALL OK" if not failed else f"{len(failed)} check(s) failed")
sys.exit(1 if failed else 0)
