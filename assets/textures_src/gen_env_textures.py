#!/usr/bin/env python3
"""R15 (Egregoria environment) — reproducible generator for the two new RKTX
environment textures.

Complements gen_cement_mud_rough.py (R7 set) and gen_ra_textures.py (R11/R12
set); same tileable value-noise approach and same RKTX writer, so this script
is deliberately self-contained (the repo keeps one file per texture batch).

Outputs (sibling ../textures/ directory, RKTX format; PNG previews stay here):
  terrain_grass.raw 256x256  terrain ground albedo, Egregoria-style grass:
                             low-frequency patch noise (8-16 lattice cells)
                             lerping between a deep green (70,100,45) and a
                             yellower green (130,150,70) around the base
                             (96,128,58) + high-frequency speckle. Kept
                             subtle/desaturated so band-color modulation on
                             the terrain mesh still reads.
                             NOTE: this is NOT the roadside grass.raw at
                             slot 1 (TEX_GRASS) — that one is baked into road
                             mesh parts and stays untouched.
  wavy.raw          128x128  tileable smooth RGB noise (4 wrap-around octaves,
                             per-channel independent), stretched to use the
                             full 0..255 range around mid-gray (128,128,128).
                             Sampled as three offset vectors for water normal
                             perturbation.

RKTX layout (must match crates/roadkit/src/textures.rs):
    b"RKTX" | u16 LE width | u16 LE height | rgba8 rows (w*h*4 bytes)

Usage:  python3 gen_env_textures.py [out_dir]
"""

import math
import os
import random
import struct
import sys

# ---------------------------------------------------------------- noise --
# Same tileable value-noise helpers as gen_cement_mud_rough.py (kept inline so
# each generator script in this directory stays runnable on its own).


def _smooth(t):
    return t * t * (3.0 - 2.0 * t)


def value_noise(rng, size, cells):
    """Single-octave value noise on a `cells` x `cells` lattice, tileable."""
    lat = [[rng.random() for _ in range(cells)] for _ in range(cells)]
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
def clamp01(v):
    return 0.0 if v < 0.0 else (1.0 if v > 1.0 else v)


def write_rktx(path, w, h, px):
    """px[y][x] = (r, g, b) floats 0..1; alpha forced 255."""
    payload = bytearray(b"RKTX")
    payload += struct.pack("<HH", w, h)
    for row in px:
        for (r, g, b) in row:
            payload += bytes((
                max(0, min(255, int(r * 255 + 0.5))),
                max(0, min(255, int(g * 255 + 0.5))),
                max(0, min(255, int(b * 255 + 0.5))),
                255,
            ))
    with open(path, "wb") as f:
        f.write(payload)
    return len(payload)


def write_png(path, w, h, px):
    """Save the same float buffer as an RGBA PNG preview (needs Pillow)."""
    try:
        from PIL import Image
    except ImportError:
        print(f"  (Pillow missing — skipped PNG preview {path})")
        return
    im = Image.new("RGBA", (w, h))
    im.putdata([
        (
            max(0, min(255, int(r * 255 + 0.5))),
            max(0, min(255, int(g * 255 + 0.5))),
            max(0, min(255, int(b * 255 + 0.5))),
            255,
        )
        for row in px for (r, g, b) in row
    ])
    im.save(path)


# ------------------------------------------------------------ generators --
def gen_terrain_grass(size=256, seed=1337):
    """Egregoria-look terrain grass albedo: patchy greens + fine speckle.

    Two-endpoint lerp does the hue work: dark (70,100,45) is a deep cold
    green, light (130,150,70) a yellower dry-patch green; the base
    (96,128,58) sits at the midpoint. Octaves stay low-frequency (8/16
    cells) so world-space UV tiling doesn't shimmer, plus per-pixel
    speckle for blade-level variation. Deliberately muted — the terrain
    renderer multiplies band colors on top.
    """
    rng = random.Random(seed)
    patch = fbm(rng, size, [(8, 0.42), (16, 0.22)])  # ~±0.32 typical
    dark = (70 / 255, 100 / 255, 45 / 255)
    light = (130 / 255, 150 / 255, 70 / 255)
    px = [[(0.0, 0.0, 0.0)] * size for _ in range(size)]
    for y in range(size):
        for x in range(size):
            t = clamp01(0.5 + patch[y][x])
            r = dark[0] + (light[0] - dark[0]) * t
            g = dark[1] + (light[1] - dark[1]) * t
            b = dark[2] + (light[2] - dark[2]) * t
            # high-frequency speckle: mostly luminance, hair of chroma
            s = (rng.random() - 0.5) * 0.055
            r += s + (rng.random() - 0.5) * 0.020
            g += s + (rng.random() - 0.5) * 0.020
            b += s * 0.75
            px[y][x] = (clamp01(r), clamp01(g), clamp01(b))
    return px


def gen_wavy(size=128, seed=4242):
    """Tileable smooth RGB noise centered on mid-gray for water normals.

    Each channel is an independent 4-octave wrap-around value noise,
    min/max-stretched to the full 0..1 range and recentered so the mean
    sits at 0.5 (value noise is symmetric, but a finite 128x128 sample
    skews a raw stretch by a few LSB). Sampling RGB then gives three
    well-scaled offset vectors in [-1, 1] around zero for normal
    perturbation. Octaves from 4 to 32 cells keep it smooth — no
    per-pixel grit — and perfectly tileable.
    """
    channels = []
    for k in range(3):
        rng = random.Random(seed + k * 101)
        n = fbm(rng, size, [(4, 0.50), (8, 0.25), (16, 0.125), (32, 0.0625)])
        lo = min(min(row) for row in n)
        hi = max(max(row) for row in n)
        inv = 1.0 / max(hi - lo, 1e-6)
        n = [[(v - lo) * inv for v in row] for row in n]
        # recenter: finite samples skew the stretch mean off 0.5 by ~0.05
        m = sum(sum(row) for row in n) / (size * size)
        channels.append([[clamp01(v - (m - 0.5)) for v in row] for row in n])
    px = [[(0.0, 0.0, 0.0)] * size for _ in range(size)]
    mean = [0.0, 0.0, 0.0]
    for y in range(size):
        for x in range(size):
            px[y][x] = (channels[0][y][x], channels[1][y][x], channels[2][y][x])
            for c in range(3):
                mean[c] += px[y][x][c]
    n = size * size
    print(f"  wavy channel means: {[round(m / n, 3) for m in mean]} (target ~0.5)")
    return px


def main():
    here = os.path.dirname(os.path.abspath(__file__))
    out = sys.argv[1] if len(sys.argv) > 1 else os.path.join(here, "..", "textures")
    os.makedirs(out, exist_ok=True)
    jobs = (
        ("terrain_grass", gen_terrain_grass, 256),
        ("wavy", gen_wavy, 128),
    )
    ok = True
    for name, fn, size in jobs:
        px = fn(size)
        raw = os.path.join(out, f"{name}.raw")
        nbytes = write_rktx(raw, size, size, px)
        expect = 8 + size * size * 4
        status = "ok" if nbytes == expect else f"BAD (expected {expect})"
        ok &= nbytes == expect
        print(f"wrote {raw} ({size}x{size}, {nbytes} bytes) {status}")
        write_png(os.path.join(here, f"{name}.png"), size, size, px)
    if not ok:
        sys.exit("RKTX size check failed")
    # header sanity: magic + dims must match textures.rs from_rktx_bytes
    for name, _, size in jobs:
        with open(os.path.join(out, f"{name}.raw"), "rb") as f:
            head = f.read(8)
        assert head[:4] == b"RKTX" and struct.unpack("<HH", head[4:8]) == (size, size), name
    print("RKTX headers verified")


if __name__ == "__main__":
    main()
