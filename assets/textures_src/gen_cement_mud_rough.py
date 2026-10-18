#!/usr/bin/env python3
"""R7-textures2 — reproducible generator for the four new RKTX textures.

Complements the R6 set (grass/sidewalk/median/asphalt, generated with the
same value-noise approach; that script was ephemeral, this one is kept).

Outputs (sibling ../textures/ directory, RKTX format):
  cement.raw      256x256  smooth light-grey cast concrete (BRT median):
                           fine value noise + faint horizontal board lines
                           + occasional dark specks + formwork tie-hole grid.
                           Lighter and smoother than median.raw.
  mud.raw         256x256  dirt-road surface: warm brown base, blotchy
                           multi-octave noise, faint wheel-rut pair (dark
                           bands running along V), scattered grit.
  rough.raw       256x256  gravel / rough aggregate: grey-brown coarse
                           speckle, high-frequency dots + a few larger
                           stones, slight vertical banding.
  red_tactile.raw 128x128  BRT station tactile paving: terracotta red with
                           a grid of small light dots (bump pattern).

RKTX layout (must match crates/roadkit/src/textures.rs):
    b"RKTX" | u16 LE width | u16 LE height | rgba8 rows (w*h*4 bytes)

Usage:  python3 gen_cement_mud_rough.py [out_dir]
"""

import math
import os
import random
import struct
import sys


# ---------------------------------------------------------------- noise --
def _lattice(rng, n):
    return [[rng.random() for _ in range(n + 1)] for _ in range(n + 1)]


def _smooth(t):
    return t * t * (3.0 - 2.0 * t)


def value_noise(rng, size, cells):
    """Single-octave value noise on a `cells` x `cells` lattice, tileable."""
    lat = _lattice(rng, cells)
    out = [[0.0] * size for _ in range(size)]
    for y in range(size):
        fy = y / size * cells
        y0 = int(fy)
        ty = _smooth(fy - y0)
        for x in range(size):
            fx = x / size * cells
            x0 = int(fx)
            tx = _smooth(fx - x0)
            a = lat[y0 % cells][x0 % cells]
            b = lat[y0 % cells][(x0 + 1) % cells]
            c = lat[(y0 + 1) % cells][x0 % cells]
            d = lat[(y0 + 1) % cells][(x0 + 1) % cells]
            top = a + (b - a) * tx
            bot = c + (d - c) * tx
            out[y][x] = top + (bot - top) * ty
    return out


def fbm(rng, size, octaves):
    """Tileable fractal value noise, octaves = [(cells, amplitude), ...]."""
    acc = [[0.0] * size for _ in range(size)]
    for cells, amp in octaves:
        n = value_noise(rng, size, cells)
        for y in range(size):
            for x in range(size):
                acc[y][x] += (n[y][x] - 0.5) * amp
    return acc


# --------------------------------------------------------------- helpers --
def write_rktx(path, size, px):
    """px[y][x] = (r, g, b) floats 0..1; alpha forced 255."""
    with open(path, "wb") as f:
        f.write(b"RKTX")
        f.write(struct.pack("<HH", size, size))
        for row in px:
            for (r, g, b) in row:
                f.write(bytes((
                    max(0, min(255, int(r * 255 + 0.5))),
                    max(0, min(255, int(g * 255 + 0.5))),
                    max(0, min(255, int(b * 255 + 0.5))),
                    255,
                )))


def clamp01(v):
    return 0.0 if v < 0.0 else (1.0 if v > 1.0 else v)


# ------------------------------------------------------------ generators --
def gen_cement(size=256, seed=71):
    """Smooth light-grey cast concrete — BRT median slab."""
    rng = random.Random(seed)
    fine = fbm(rng, size, [(6, 0.030), (24, 0.020)])
    blotch = value_noise(rng, size, 3)  # very low-freq tonal drift
    px = [[(0.0, 0.0, 0.0)] * size for _ in range(size)]
    for y in range(size):
        for x in range(size):
            v = 0.645 + fine[y][x] + (blotch[y][x] - 0.5) * 0.05
            # faint horizontal board (formwork) lines every 36 px
            board = y % 36
            if board <= 1:
                v -= 0.045 * (1.0 if board == 0 else 0.5)
            px[y][x] = (v, v, min(1.0, v * 1.015 + 0.012))  # hair cool
    # occasional dark specks
    for _ in range(150):
        x, y = rng.randrange(size), rng.randrange(size)
        d = rng.uniform(0.06, 0.14)
        r, g, b = px[y][x]
        px[y][x] = (r - d, g - d, b - d * 0.9)
    # formwork tie-hole grid: dark circles every 64 px, offset (32, 32)
    for gy in range(32, size, 64):
        for gx in range(32, size, 64):
            for dy in range(-3, 4):
                for dx in range(-3, 4):
                    d2 = dx * dx + dy * dy
                    if d2 <= 9:
                        x, y = (gx + dx) % size, (gy + dy) % size
                        fall = 1.0 - math.sqrt(d2) / 3.0
                        r, g, b = px[y][x]
                        k = 0.16 * fall
                        px[y][x] = (r - k, g - k, b - k * 0.9)
    return px


def gen_mud(size=256, seed=23):
    """Dirt road: warm brown, blotchy, wheel-rut pair along V, grit."""
    rng = random.Random(seed)
    blotch = fbm(rng, size, [(4, 0.16), (8, 0.10), (16, 0.06), (48, 0.035)])
    px = [[(0.0, 0.0, 0.0)] * size for _ in range(size)]
    for y in range(size):
        for x in range(size):
            u = x / size
            # multiplicative blotches (wet/dry patches)
            mod = 1.0 + blotch[y][x] * 2.2
            # faint wheel-rut pair: dark bands running along V (constant u)
            for rut_u in (0.30, 0.70):
                d = abs(u - rut_u)
                if d < 0.10:
                    mod *= 1.0 - 0.16 * _smooth(1.0 - d / 0.10)
            # slightly lighter crown between the ruts
            if 0.42 < u < 0.58:
                mod *= 1.0 + 0.05 * (1.0 - abs(u - 0.5) / 0.08)
            r = 0.46 * mod
            g = 0.345 * mod
            b = 0.235 * mod
            px[y][x] = (clamp01(r), clamp01(g), clamp01(b))
    # scattered grit: light dry specks + dark clods
    for _ in range(900):
        x, y = rng.randrange(size), rng.randrange(size)
        r, g, b = px[y][x]
        if rng.random() < 0.55:
            k = rng.uniform(0.05, 0.12)
            px[y][x] = (clamp01(r + k), clamp01(g + k * 0.8), clamp01(b + k * 0.55))
        else:
            k = rng.uniform(0.05, 0.10)
            px[y][x] = (clamp01(r - k), clamp01(g - k), clamp01(b - k * 0.8))
    return px


def gen_rough(size=256, seed=47):
    """Gravel / rough aggregate: grey-brown speckle, stones, banding."""
    rng = random.Random(seed)
    clump = fbm(rng, size, [(8, 0.075), (32, 0.045)])
    px = [[(0.0, 0.0, 0.0)] * size for _ in range(size)]
    for y in range(size):
        for x in range(size):
            u = x / size
            mod = 1.0 + clump[y][x] * 2.0
            # slight vertical banding (runs along V)
            mod *= 1.0 + 0.035 * math.sin(u * math.tau * 6.0 + 0.7)
            # high-frequency per-pixel speckle
            sp = (rng.random() - 0.5) * 0.17
            r = 0.40 * mod + sp
            g = 0.37 * mod + sp * 0.95
            b = 0.33 * mod + sp * 0.85
            px[y][x] = (clamp01(r), clamp01(g), clamp01(b))
    # a few larger stones: soft round light/dark patches
    for _ in range(46):
        cx, cy = rng.randrange(size), rng.randrange(size)
        rad = rng.uniform(1.8, 4.2)
        light = rng.random() < 0.6
        k = rng.uniform(0.10, 0.20) * (1.0 if light else -1.0)
        ir = int(math.ceil(rad))
        for dy in range(-ir, ir + 1):
            for dx in range(-ir, ir + 1):
                d = math.hypot(dx, dy)
                if d <= rad:
                    x, y = (cx + dx) % size, (cy + dy) % size
                    fall = _smooth(1.0 - d / rad)
                    r, g, b = px[y][x]
                    px[y][x] = (
                        clamp01(r + k * fall),
                        clamp01(g + k * fall * 0.92),
                        clamp01(b + k * fall * 0.8),
                    )
    return px


def gen_red_tactile(size=128, seed=91):
    """BRT tactile paving: terracotta red + grid of small light dots."""
    rng = random.Random(seed)
    fine = fbm(rng, size, [(8, 0.030), (32, 0.020)])
    px = [[(0.0, 0.0, 0.0)] * size for _ in range(size)]
    for y in range(size):
        for x in range(size):
            v = fine[y][x]
            r = 0.685 + v
            g = 0.330 + v * 0.8
            b = 0.255 + v * 0.7
            px[y][x] = (clamp01(r), clamp01(g), clamp01(b))
    # dot bump grid every 16 px: light dome + darker rim
    step = 16
    rad = 3.2
    ir = int(math.ceil(rad)) + 1
    for gy in range(step // 2, size + step // 2, step):
        for gx in range(step // 2, size + step // 2, step):
            for dy in range(-ir, ir + 1):
                for dx in range(-ir, ir + 1):
                    d = math.hypot(dx, dy)
                    x, y = (gx + dx) % size, (gy + dy) % size
                    r, g, b = px[y][x]
                    if d <= rad:
                        k = 0.17 * _smooth(1.0 - d / rad)
                        px[y][x] = (clamp01(r + k), clamp01(g + k * 0.85), clamp01(b + k * 0.7))
                    elif d <= rad + 1.2:
                        k = 0.06 * (1.0 - (d - rad) / 1.2)
                        px[y][x] = (clamp01(r - k), clamp01(g - k), clamp01(b - k))
    return px


def main():
    here = os.path.dirname(os.path.abspath(__file__))
    out = sys.argv[1] if len(sys.argv) > 1 else os.path.join(here, "..", "textures")
    os.makedirs(out, exist_ok=True)
    jobs = (
        ("cement.raw", gen_cement, 256),
        ("mud.raw", gen_mud, 256),
        ("rough.raw", gen_rough, 256),
        ("red_tactile.raw", gen_red_tactile, 128),
    )
    for name, fn, size in jobs:
        px = fn(size)
        write_rktx(os.path.join(out, name), size, px)
        print(f"wrote {os.path.join(out, name)} ({size}x{size})")


if __name__ == "__main__":
    main()
