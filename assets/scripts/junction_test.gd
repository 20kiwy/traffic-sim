# RoadKit Godot — junction auto-detection test
# Builds roads that MEET and CROSS so the auto-join engine detects
# junctions like the Godot plugin's demo gifs:
#   1. two roads CROSSING  -> 4-way n-gon intersection (GRG IntersectionNGon)
#   2. three roads MEETING -> roundabout ring (the custom-container prefab)
#   3. one road ENDING on another -> T-junction
# All junctions are detected automatically — no manual "Create intersection".

extends Node3D

func mkroad(name, x1, z1, x2, z2):
        var curve = RoadCurve.new(name)
        var a = RoadPoint.new()
        a.position = Vector3(x1, 0, z1)
        a.bearing = atan2(z2 - z1, x2 - x1)
        a.lanes = 2
        a.next_mag = 50.0
        curve.add_point(a)
        var b = RoadPoint.new()
        b.position = Vector3(x2, 0, z2)
        b.bearing = atan2(z2 - z1, x2 - x1)
        b.lanes = 2
        b.prior_mag = 50.0
        curve.add_point(b)
        curve.build_mesh()
        return curve

func _ready():
        print("RoadKit: junction test — crossing + roundabout + T-join...")
        auto_junction(true)
        auto_roundabout(true)
        set_snap_m(8.0)

        # 1. four-way crossing at the origin (pure THROUGH crossing)
        mkroad("North-South", 0, -140, 0, 140)
        mkroad("East-West", -140, 0, 140, 0)

        # 2. three roads meeting at (260, 0) -> roundabout
        mkroad("Radial A", 120, -110, 260, 0)
        mkroad("Radial B", 150, 100, 260, 0)
        mkroad("Radial C", 380, -30, 260, 0)

        # 3. a side road ending ON the East-West body -> T junction
        mkroad("Side Rd", 60, 90, 60, 0)

        print(junction_info())
        print("Junction test done — open the 2D Plan tab to see the snapping.")
