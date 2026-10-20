#!/usr/bin/env python3
"""Generate workzone prop GLBs (binary glTF 2.0) for the roadkit library:
  cone.glb       — traffic cone ~0.7 m: square base, 8-seg frustum body,
                   white reflective band
  barrel.glb     — workzone channelizer drum ~0.9 m: 16-seg cylinder with
                   two white horizontal bands
  attenuator.glb — impact attenuator wedge: 5 stacked box stages, yellow
                   with black reflective stripes
All parts carry vertex colors (lambert-shaded by the engine rasterizer).
Run from the repo root:  python3 assets/glb_src/gen_roadwork.py
"""
import json
import math
import struct
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "assets" / "glb"

ORANGE = (230 / 255, 90 / 255, 30 / 255, 1.0)
WHITE = (0.92, 0.92, 0.90, 1.0)
YELLOW = (230 / 255, 190 / 255, 40 / 255, 1.0)
BLACK = (0.09, 0.09, 0.10, 1.0)
RUBBER = (0.16, 0.16, 0.17, 1.0)


def rgba(c):
    return c if len(c) == 4 else (c[0], c[1], c[2], 1.0)


def frustum(r0, r1, y0, y1, sides, color, caps=("bottom", "top")):
    """Truncated cone (side quads + optional cap discs)."""
    pos, idx, cols = [], [], []
    for (r, y) in [(r0, y0), (r1, y1)]:
        for s in range(sides):
            a = 2 * math.pi * s / sides
            pos.append([math.cos(a) * r, y, math.sin(a) * r])
            cols.append(rgba(color))
    for s in range(sides):
        a0, a1 = s, (s + 1) % sides
        idx += [a0, a1, sides + a1, a0, sides + a1, sides + a0]
    for cap, r, y in (("bottom", r0, y0), ("top", r1, y1)):
        if cap not in caps or r <= 0.0:
            continue
        cbase = len(pos)
        pos.append([0.0, y, 0.0]); cols.append(rgba(color))
        for s in range(sides):
            a = 2 * math.pi * s / sides
            pos.append([math.cos(a) * r, y, math.sin(a) * r])
            cols.append(rgba(color))
        for s in range(sides):
            if cap == "top":
                idx += [cbase, cbase + 1 + s, cbase + 1 + (s + 1) % sides]
            else:
                idx += [cbase, cbase + 1 + (s + 1) % sides, cbase + 1 + s]
    return pos, idx, cols


def aabb_box(x0, x1, y0, y1, z0, z1, color):
    """Axis-aligned box (8 verts, 12 tris)."""
    pos = [
        [x0, y0, z0], [x1, y0, z0], [x1, y1, z0], [x0, y1, z0],  # back z0
        [x0, y0, z1], [x1, y0, z1], [x1, y1, z1], [x0, y1, z1],  # front z1
    ]
    cols = [rgba(color)] * 8
    idx = [
        0, 1, 2, 0, 2, 3,   # back
        5, 4, 7, 5, 7, 6,   # front
        4, 0, 3, 4, 3, 7,   # left
        1, 5, 6, 1, 6, 2,   # right
        3, 2, 6, 3, 6, 7,   # top
        4, 5, 1, 4, 1, 0,   # bottom
    ]
    return pos, idx, cols


def write_glb(path, prims):
    """prims: list of (positions, indices, colors, material_name)."""
    blob = bytearray()
    buffer_views, accessors, materials, mesh_prims = [], [], [], []

    def add_view(data, target=None):
        off = len(blob)
        blob.extend(data)
        while len(blob) % 4:
            blob.append(0)
        v = {"buffer": 0, "byteOffset": off, "byteLength": len(data)}
        if target:
            v["target"] = target
        buffer_views.append(v)
        return len(buffer_views) - 1

    for (pos, idx, cols, mat_name) in prims:
        mi = len(materials)
        materials.append({
            "name": mat_name,
            "pbrMetallicRoughness": {"baseColorFactor": [1, 1, 1, 1]},
            "doubleSided": True,
        })
        pv = add_view(struct.pack(f"<{len(pos)*3}f", *[c for p in pos for c in p]), 34962)
        pa = {"bufferView": pv, "componentType": 5126, "count": len(pos), "type": "VEC3",
              "min": [min(p[0] for p in pos), min(p[1] for p in pos), min(p[2] for p in pos)],
              "max": [max(p[0] for p in pos), max(p[1] for p in pos), max(p[2] for p in pos)]}
        accessors.append(pa); ai = len(accessors) - 1
        cv = add_view(struct.pack(f"<{len(cols)*4}f", *[c for col in cols for c in col]), 34962)
        accessors.append({"bufferView": cv, "componentType": 5126, "count": len(cols), "type": "VEC4"})
        ci = len(accessors) - 1
        iv = add_view(struct.pack(f"<{len(idx)}I", *idx), 34963)
        accessors.append({"bufferView": iv, "componentType": 5125, "count": len(idx), "type": "SCALAR"})
        ii = len(accessors) - 1
        mesh_prims.append({
            "attributes": {"POSITION": ai, "COLOR_0": ci},
            "indices": ii, "material": mi, "mode": 4,
        })
    meshes = [{"primitives": mesh_prims, "name": Path(path).stem}]
    nodes = [{"mesh": 0, "name": Path(path).stem}]
    gltf = {
        "asset": {"version": "2.0", "generator": "traffic-sim gen_roadwork.py"},
        "scene": 0,
        "scenes": [{"nodes": [0]}],
        "nodes": nodes,
        "meshes": meshes,
        "materials": materials,
        "accessors": accessors,
        "bufferViews": buffer_views,
        "buffers": [{"byteLength": len(blob)}],
    }
    js = json.dumps(gltf, separators=(",", ":")).encode()
    while len(js) % 4:
        js += b" "
    out = struct.pack("<III", 0x46546C67, 2, 12 + 8 + len(js) + 8 + len(blob))
    out += struct.pack("<I4s", len(js), b"JSON") + js
    out += struct.pack("<I4s", len(blob), b"BIN\0") + bytes(blob)
    Path(path).write_bytes(out)
    print(f"{path}  {len(out)} bytes  {len(prims)} prims  "
          f"{sum(len(p[1])//3 for p in prims)} tris")
    return len(out)


def gen_cone():
    """Traffic cone ~0.7 m: square base 0.36 m + orange 8-seg frustum body
    with a white reflective band (~120 tris)."""
    prims = []
    # square base plate + a low rubber step
    prims.append((*aabb_box(-0.18, 0.18, 0.0, 0.022, -0.18, 0.18, ORANGE), "base"))
    prims.append((*aabb_box(-0.15, 0.15, 0.022, 0.05, -0.15, 0.15, RUBBER), "base"))
    # body in three frusta: orange / white band / orange tip
    prims.append((*frustum(0.155, 0.105, 0.05, 0.40, 8, ORANGE), "body"))
    prims.append((*frustum(0.112, 0.098, 0.40, 0.52, 8, WHITE), "band"))
    prims.append((*frustum(0.105, 0.035, 0.52, 0.70, 8, ORANGE, caps=("top",)), "body"))
    return prims


def gen_barrel():
    """Workzone channelizer drum ~0.9 m tall: 16-seg cylinder r=0.28,
    orange with two white horizontal bands."""
    prims = []
    bands = [(0.02, 0.27, ORANGE), (0.27, 0.37, WHITE), (0.37, 0.55, ORANGE),
             (0.55, 0.65, WHITE), (0.65, 0.88, ORANGE)]
    for i, (y0, y1, col) in enumerate(bands):
        pad = 0.012 if col == WHITE else 0.0
        caps = ("bottom",) if i == 0 else ()
        prims.append((*frustum(0.28 + pad, 0.28 + pad, y0, y1, 16, col, caps=caps), "drum"))
    # top lid (slight dome step) + bottom ring
    prims.append((*frustum(0.28, 0.24, 0.88, 0.92, 16, ORANGE, caps=("top",)), "drum"))
    prims.append((*frustum(0.30, 0.28, 0.0, 0.02, 16, RUBBER, caps=("bottom",)), "base"))
    return prims


def gen_attenuator():
    """Impact attenuator wedge: 5 stacked box stages along +x, growing
    toward the barrier, yellow with black reflective stripe bands.
    Overall ~1.6 m long x 0.8 m wide x 0.85 m tall."""
    prims = []
    n = 5
    length, w0, w1, h0, h1 = 1.6, 0.30, 0.80, 0.25, 0.85
    seg = length / n
    for i in range(n):
        t = i / (n - 1)
        w = w0 + (w1 - w0) * t
        h = h0 + (h1 - h0) * t
        x0, x1 = i * seg, (i + 1) * seg
        # shell stage (open look via slightly inset core + full shell)
        prims.append((*aabb_box(x0, x1, 0.0, h, -w / 2, w / 2, YELLOW), "stage"))
        # black reflective stripe band around the stage
        band_x0 = x0 + seg * 0.28
        band_x1 = band_x0 + seg * 0.2
        prims.append((*aabb_box(band_x0, band_x1, h * 0.55, h * 0.78,
                                -w / 2 - 0.012, w / 2 + 0.012, BLACK), "stripe"))
    # dark skid base
    prims.append((*aabb_box(0.0, length, 0.0, 0.035, -w1 / 2 - 0.02, w1 / 2 + 0.02, RUBBER), "base"))
    return prims


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    sizes = {}
    sizes["cone"] = write_glb(OUT / "cone.glb", gen_cone())
    sizes["barrel"] = write_glb(OUT / "barrel.glb", gen_barrel())
    sizes["attenuator"] = write_glb(OUT / "attenuator.glb", gen_attenuator())
    # keep manifest.json in sync (id/file/bytes/role entries)
    man = OUT / "manifest.json"
    if man.exists():
        data = json.loads(man.read_text())
        roles = {
            "cone": "workzone — traffic cone",
            "barrel": "workzone — channelizer drum",
            "attenuator": "workzone — impact attenuator",
        }
        assets = [a for a in data.get("assets", []) if a.get("id") not in sizes]
        for name in ["cone", "barrel", "attenuator"]:
            assets.append({
                "id": name,
                "file": f"assets/glb/{name}.glb",
                "bytes": sizes[name],
                "role": roles[name],
            })
        data["assets"] = assets
        man.write_text(json.dumps(data, indent=1) + "\n")
        print(f"manifest.json updated ({len(assets)} assets)")


if __name__ == "__main__":
    main()
