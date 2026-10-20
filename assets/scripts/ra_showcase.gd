# RoadKit Godot — RA systems showcase (R12)
# One scene exercising every RoadArchitect visual system ported to Rust:
#   1. twin 4-lane carriageway with median cable barrier (GSDCableBarrier:
#      posts every 4 m + 3 steel cables per side) + W-beam edges
#   2. suspension bridge kit (bridge_style = "suspension": towers every
#      150 m, catenary main cables with 16 m sag, hangers every 5 m)
#   3. semi-arch bridge kit (bridge_style = "arch": twin concrete ribs +
#      spandrel columns under the deck)
#   4. GSDTunnel (tunnel = true: vault + portal faces + crown strip lights)
#   5. workzone pack: traffic cones, channelizer drums, impact attenuator

extends Node3D

func _ready():
        print("RoadKit: building RA systems showcase...")
        # rolling ground so the tunnel + bridges read against raised terrain
        set_terrain(true)
        set_terrain_hills(5.0, 170.0)

        # ---- 1. twin carriageway + median cable barrier -----------------
        # 400 m straight, median 2.4 m; the cable barrier lives IN the
        # median, W-beam guardrails run both outer edges.
        var tw = RoadCurve.new("RA Twin Carriageway")
        for i in range(5):
                var p = RoadPoint.new()
                p.position = Vector3(-200 + i * 100, 0, 0)
                p.auto_bearing = true
                p.prior_mag = 60.0
                p.next_mag = 60.0
                p.lanes = 4
                p.median = 2.4
                tw.add_point(p)
        tw.guardrail = true
        tw.cable_barrier = true
        tw.build_mesh()
        tw.add_props("streetlight", 36.0, true)

        # ---- 2. suspension bridge (deck y=9, bridges skip terrain) ------
        var sus = RoadCurve.new("RA Suspension Bridge")
        for i in range(7):
                var p = RoadPoint.new()
                p.position = Vector3(-240 + i * 80, 9, 150)
                p.auto_bearing = true
                p.lanes = 2
                p.flatten_terrain = false
                sus.add_point(p)
        sus.bridge_style = "suspension"
        sus.build_mesh()

        # ---- 3. semi-arch bridge (lower deck y=5, north side) -----------
        var arch = RoadCurve.new("RA Arch Bridge")
        for i in range(6):
                var p = RoadPoint.new()
                p.position = Vector3(-150 + i * 70, 5, -150)
                p.auto_bearing = true
                p.lanes = 2
                p.flatten_terrain = false
                arch.add_point(p)
        arch.bridge_style = "arch"
        arch.build_mesh()

        # ---- 4. tunnel through the raised ground (points at y>0) --------
        var tun = RoadCurve.new("RA Tunnel 4L")
        for i in range(6):
                var p = RoadPoint.new()
                p.position = Vector3(-150 + i * 70, 2.2, -60)
                p.auto_bearing = true
                p.lanes = 4
                p.flatten_terrain = false
                tun.add_point(p)
        tun.tunnel = true
        tun.build_mesh()

        # ---- 5. workzone on the twin carriageway west end ---------------
        # cones every 6 m alternating verges, drums every 18 m on the
        # east verge, one impact attenuator per workzone entry.
        tw.add_props_offset("cone", 6.0, 7.0, true, true)
        tw.add_props_offset("barrel", 18.0, 8.5, false, true)
        tw.stamp_prop("attenuator", 0.02, 3.0)
        tw.stamp_prop("attenuator", 0.30, 3.0)
        tw.stamp_prop("signRoadName", 0.01, 6.5)

        print("RA showcase: " + str(tw.length()) + " m twin carriageway + suspension + arch + tunnel")
        print("RoadKit: RA systems showcase built — see the 3D Scene tab")
