#!/usr/bin/env python3
"""Convert RoadArchitect textures (assets/ra_textures/*.png) + a procedural
window facade into RKTX raw files (magic 'RKTX' + u16 w + u16 h + rgba8).

Slots appended after the 9 standard ones (see roadkit/src/textures.rs):
  ra_asphalt, ra_concrete, ra_galv, ra_wbeam, ra_railing, ra_brick,
  ra_cobble, windows
Run from the repo root:  python3 assets/textures_src/gen_ra_textures.py
"""
import struct
from pathlib import Path

from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parents[2]
RA = ROOT / "assets" / "ra_textures"
OUT = ROOT / "assets" / "textures"

FILES = {
    "ra_asphalt": "GSDRoad1_Diffuse.png",
    "ra_concrete": "GSDConcrete1_Diffuse.png",
    "ra_galv": "GSDGalv1_Diffuse.png",
    "ra_wbeam": "GSDWBeam_Diffuse.png",
    "ra_railing": "GSDRailing4_Diffuse.png",
    "ra_brick": "GSDBrickRoad_Diffuse.png",
    "ra_cobble": "GSDCobblestoneRoad_Diffuse.png",
}


def save_rktx(name: str, im: Image.Image) -> None:
    im = im.convert("RGBA")
    data = im.tobytes()
    payload = b"RKTX" + struct.pack("<HH", im.width, im.height) + data
    (OUT / f"{name}.raw").write_bytes(payload)
    print(f"{name}.raw  {im.width}x{im.height}  {len(payload)} bytes")


def gen_windows(size: int = 256) -> Image.Image:
    """Procedural facade: concrete background + lit/unlit window grid.

    One texture tile = one storey (v) x ~4 m of facade (u): 2 windows per
    tile with sills, staggered lit state — tiled per floor by buildings.rs.
    """
    im = Image.new("RGBA", (size, size), (168, 162, 152, 255))
    d = ImageDraw.Draw(im)
    # subtle concrete noise
    import random

    rnd = random.Random(7)
    for _ in range(900):
        x, y = rnd.randrange(size), rnd.randrange(size)
        g = rnd.randint(-14, 14)
        d.point((x, y), fill=(168 + g, 162 + g, 152 + g, 255))
    # floor slab band along the bottom (v near 1)
    d.rectangle([0, size - size // 8, size, size], fill=(140, 136, 128, 255))
    # two windows
    margin = size // 10
    win_w = (size - 3 * margin) // 2
    win_h = int(size * 0.62)
    for i in range(2):
        x0 = margin + i * (win_w + margin)
        y0 = int(size * 0.12)
        # frame
        d.rectangle([x0 - 3, y0 - 3, x0 + win_w + 3, y0 + win_h + 3], fill=(96, 92, 86, 255))
        # glass — half the windows lit (warm), half dark (sky reflection)
        lit = (i % 2 == 0) ^ (rnd.random() < 0.5)
        if lit:
            glass = (214, 206, 168, 255)
        else:
            glass = (62, 74, 84, 255)
        d.rectangle([x0, y0, x0 + win_w, y0 + win_h], fill=glass)
        # mullion + reflection streak
        d.rectangle([x0 + win_w // 2 - 1, y0, x0 + win_w // 2 + 1, y0 + win_h], fill=(96, 92, 86, 255))
        d.line([x0 + 4, y0 + win_h - 8, x0 + win_w // 3, y0 + 6], fill=(255, 255, 255, 40), width=3)
        # sill
        d.rectangle([x0 - 5, y0 + win_h + 3, x0 + win_w + 5, y0 + win_h + 7], fill=(120, 116, 108, 255))
    return im


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    for name, fname in FILES.items():
        p = RA / fname
        if not p.exists():
            print(f"SKIP {name}: {p.name} missing")
            continue
        im = Image.open(p)
        # keep sizes modest for wasm embedding (1024 -> keep asphalt, rest 512)
        if name != "ra_asphalt" and max(im.size) > 512:
            im = im.resize((512, int(im.height * 512 / im.width)))
        save_rktx(name, im)
    save_rktx("windows", gen_windows())


if __name__ == "__main__":
    main()
