# RoadKit Godot — grid network
# Nested for-loops generate a whole road grid (the scripting answer to
# procedural network generation): rows east-west, columns north-south,
# then streetlights and trees join along every road.

extends Node3D

func _ready():
	print("RoadKit: generating grid network...")
	var step = 120.0
	var cols = 4
	var rows = 3

	for r in range(rows + 1):
		var row = RoadCurve.new("Row " + str(r))
		for c in range(cols + 1):
			var p = RoadPoint.new()
			p.position = Vector3(c * step, 0, r * step)
			p.bearing = 0.0
			p.prior_mag = 40.0
			p.next_mag = 40.0
			p.lanes = 2
			row.add_point(p)
		row.build_mesh()
		row.add_props("streetlight", 60.0, true)

	for c in range(cols + 1):
		var col = RoadCurve.new("Col " + str(c))
		for r in range(rows + 1):
			var p = RoadPoint.new()
			p.position = Vector3(c * step, 0, r * step)
			p.bearing = deg_to_rad(90.0)
			p.prior_mag = 40.0
			p.next_mag = 40.0
			p.lanes = 2
			col.add_point(p)
		col.build_mesh()
		col.add_props("tree", 45.0, true)

	print("Grid network: " + str(rows + 1 + cols + 1) + " roads built — open the 3D Scene")
