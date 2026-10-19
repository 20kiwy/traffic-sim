# RoadKit Godot — TerraZone grid verification: terrain + hills + roads

extends Node3D

func _ready():
	print("RoadKit: terrain grid test...")
	set_terrain(true)
	set_terrain_hills(9.0, 1.0)

	var step = 120.0
	for r in range(3):
		var row = RoadCurve.new("TRow " + str(r))
		for c in range(4):
			var p = RoadPoint.new()
			p.position = Vector3(c * step, 0, r * step)
			p.bearing = 0.0
			p.prior_mag = 40.0
			p.next_mag = 40.0
			p.lanes = 2
			row.add_point(p)
		row.build_mesh()
