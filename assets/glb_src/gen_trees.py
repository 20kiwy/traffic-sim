#!/usr/bin/env python3
"""Generate tree GLBs (binary glTF 2.0) for the roadkit asset library:
  tree.glb  — African acacia: tapered trunk + wide layered flat canopy
  palm.glb  — coconut palm: curved trunk + radiating frond blades
Both are multi-primitive (per-part vertex colors, lambert-shaded by the
engine rasterizer). Run from the repo root:
  python3 assets/glb_src/gen_trees.py
"""
import json
import math
import random
import struct
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "assets" / "glb"


def cylinder(r_bottom, r_top, y0, y1, sides, color, lean=(0.0, 0.0)):
    """Tapered tube; returns (positions, indices, colors)."""
    pos, idx, cols = [], [], []
    rings = 2
    for r in range(rings):
        t = r / (rings - 1)
        y = y0 + (y1 - y0) * t
        rad = r_bottom + (r_top - r_bottom) * t
        cx = lean[0] * t * t
        cz = lean[1] * t * t
        for s in range(sides):
            a = 2 * math.pi * s / sides
            pos.append([cx + math.cos(a) * rad, y, cz + math.sin(a) * rad])
            cols.append(color)
    for s in range(sides):
        a0, a1 = s, (s + 1) % sides
        idx += [a0, a1, sides + a1, a0, sides + a1, sides + a0]
    # cap disc (top)
    cbase = len(pos)
    pos.append([lean[0], y1, lean[1]])
    cols.append(color)
    for s in range(sides):
        a = 2 * math.pi * s / sides
        rad = r_top
        pos.append([lean[0] + math.cos(a) * rad, y1, lean[1] + math.sin(a) * rad])
        cols.append(color)
    for s in range(sides):
        idx += [cbase, cbase + 1 + s, cbase + 1 + (s + 1) % sides]
    return pos, idx, cols


def canopy_disc(cx, cy, cz, r_in, r_out, h, sides, color, rnd, sag=0.35):
    """Irregular flat canopy layer (drooping rim — acacia umbrella)."""
    pos, idx, cols = [], [], []
    wob = [rnd.uniform(0.72, 1.15) for _ in range(sides)]
    pos.append([cx, cy + h * 0.35, cz]); cols.append(color)
    for s in range(sides):
        a = 2 * math.pi * s / sides
        pos.append([cx + math.cos(a) * r_in * wob[s], cy + h * 0.15, cz + math.sin(a) * r_in * wob[s]])
        cols.append(color)
    for s in range(sides):
        a = 2 * math.pi * s / sides
        pos.append([cx + math.cos(a) * r_out * wob[s], cy - h * sag, cz + math.sin(a) * r_out * wob[s]])
        cols.append(color)
    for s in range(sides):
        s1 = (s + 1) % sides
        idx += [0, 1 + s, 1 + s1]
        idx += [1 + s, 1 + sides + s, 1 + sides + s1]
        idx += [1 + s, 1 + sides + s1, 1 + s1]
    return pos, idx, cols


def frond(cx, cy, cz, yaw, length, color, tilt=0.5):
    """One palm frond: a bent blade (3 segments drooping)."""
    pos, idx, cols = [], [], []
    ca, sa = math.cos(yaw), math.sin(yaw)
    width0, width1 = 0.34, 0.06
    segs = 3
    for i in range(segs + 1):
        t = i / segs
        d = length * t
        drop = length * tilt * t * t
        w = width0 + (width1 - width0) * t
        px, py = cx + ca * d, cy - drop
        pz = cz + sa * d
        pos.append([px - sa * w, py, pz + ca * w]); cols.append(color)
        pos.append([px + sa * w, py, pz - ca * w]); cols.append(color)
    for i in range(segs):
        b = i * 2
        idx += [b, b + 1, b + 3, b, b + 3, b + 2]
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
        "asset": {"version": "2.0", "generator": "traffic-sim gen_trees.py"},
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


def gen_acacia(seed=11):
    rnd = random.Random(seed)
    greens = [
        (0.32, 0.46, 0.22, 1.0),
        (0.38, 0.52, 0.25, 1.0),
        (0.28, 0.42, 0.20, 1.0),
    ]
    trunk = (0.36, 0.27, 0.19, 1.0)
    prims = []
    p, i, c = cylinder(0.22, 0.14, 0.0, 2.2, 9, trunk, lean=(0.06, 0.04))
    prims.append((p, i, c, "bark"))
    for lean, h0, h1, rb, rt in [((0.55, 0.15), 2.0, 3.4, 0.13, 0.07),
                                 ((-0.42, 0.38), 2.1, 3.2, 0.11, 0.06),
                                 ((0.1, -0.6), 1.9, 3.0, 0.10, 0.05)]:
        p, i, c = cylinder(rb, rt, h0, h1, 7, trunk, lean=lean)
        prims.append((p, i, c, "bark"))
    tops = [(0.45, 3.5, 4.6, 2.6), (-0.4, 4.1, 3.4, 2.2), (0.2, 4.55, 2.2, 1.6)]
    for k, (cx, cy, r_out, h) in enumerate(tops):
        p, i, c = canopy_disc(cx, cy, 0.1 * k, r_out * 0.55, r_out, h, 14,
                              greens[k % len(greens)], rnd, sag=0.5)
        prims.append((p, i, c, "canopy"))
    return prims


def gen_palm(seed=5):
    rnd = random.Random(seed)
    trunk = (0.42, 0.33, 0.22, 1.0)
    green = (0.30, 0.50, 0.24, 1.0)
    dark = (0.24, 0.42, 0.20, 1.0)
    prims = []
    lean = (rnd.uniform(-0.9, 0.9), rnd.uniform(-0.9, 0.9))
    p, i, c = cylinder(0.16, 0.10, 0.0, 5.2, 8, trunk, lean=lean)
    prims.append((p, i, c, "bark"))
    for h in [1.2, 2.4, 3.6]:
        p, i, c = cylinder(0.135, 0.125, h, h + 0.18, 8, dark, lean=lean)
        prims.append((p, i, c, "bark"))
    top_x, top_z = lean[0], lean[1]
    top_y = 5.2
    n = 9
    for k in range(n):
        yaw = 2 * math.pi * k / n + rnd.uniform(-0.15, 0.15)
        col = green if k % 2 == 0 else dark
        p, i, c = frond(top_x, top_y, top_z, yaw, rnd.uniform(2.6, 3.4), col,
                        tilt=rnd.uniform(0.55, 0.8))
        prims.append((p, i, c, "frond"))
    for k in range(3):
        a = 2.2 * k
        p, i, c = cylinder(0.11, 0.11, top_y - 0.25, top_y - 0.05, 6, (0.35, 0.26, 0.14, 1.0))
        p = [[x + top_x + math.cos(a) * 0.2, y, z + top_z + math.sin(a) * 0.2] for x, y, z in p]
        prims.append((p, i, c, "coconut"))
    return prims


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    write_glb(OUT / "tree.glb", gen_acacia())
    write_glb(OUT / "palm.glb", gen_palm())


if __name__ == "__main__":
    main()
