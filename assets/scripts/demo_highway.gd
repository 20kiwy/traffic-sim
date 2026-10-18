# RoadKit Godot — demo highway
# Builds a curved 2-lane highway from three RoadPoints (GRG style) and
# JOINS the Godot GLB props along it: streetlights alternate sides,
# trees line the verges, milestones every 120 m, plus stamped vehicles
# and junction markers — exactly how godot-road-generator dresses roads.

extends Node3D

func _ready():
        print("RoadKit: building demo highway...")
        var curve = RoadCurve.new("Highway A1")

        var a = RoadPoint.new()
        a.position = Vector3(-120, 0, -30)
        a.auto_bearing = true
        a.lanes = 2
        a.next_mag = 60.0
        curve.add_point(a)

        var b = RoadPoint.new()
        b.position = Vector3(0, 0, 30)
        b.auto_bearing = true
        b.lanes = 2
        b.prior_mag = 60.0
        b.next_mag = 60.0
        curve.add_point(b)

        var c = RoadPoint.new()
        c.position = Vector3(120, 0, -20)
        c.auto_bearing = true
        c.lanes = 2
        c.prior_mag = 60.0
        curve.add_point(c)

        curve.build_mesh()
        print("Highway A1 length: " + str(curve.length()) + " m")

        # Join GLB props along the curve — the Godot Road Generator way:
        curve.add_props("streetlight", 30.0, true)
        curve.add_props("tree", 16.0, true)
        curve.add_props("milestone", 120.0, true)
        curve.stamp_prop("vehicle", 0.30, 1.75)
        curve.stamp_prop("vehicle", 0.62, -1.75)
        curve.stamp_prop("junctionMarker", 0.0, 0.0)
        curve.stamp_prop("junctionMarker", 1.0, 0.0)

        print("RoadKit: props joined along curve — see the 3D Scene tab")
