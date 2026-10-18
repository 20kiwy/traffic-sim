# Scenario: city grid — 5x4 blocks, arterials with medians + sidewalks,
# collectors plain, streetlights + trees + vehicles joined along every road.
# A dense junction field: every crossing is auto-detected as an n-gon.

extends Node3D

func _ready():
	print("RoadKit: building city grid...")
	var step = 110.0
	var cols = 4
	var rows = 3

	# east-west arterials (rows): every other one gets a median + sidewalks
	for r in range(rows + 1):
		var row = RoadCurve.new("Ave " + str(r + 1))
		var arterial = r % 2 == 0
		for c in range(cols + 1):
			var p = RoadPoint.new()
			p.position = Vector3(c * step, 0, r * step)
			p.bearing = 0.0
			p.prior_mag = 38.0
			p.next_mag = 38.0
			if arterial:
				p.lanes = 4
			else:
				p.lanes = 2
			if arterial:
				p.median = 2.0
				p.pathway_l = 2.5
				p.pathway_r = 2.5
			row.add_point(p)
		row.build_mesh()
		row.add_props("streetlight", 55.0, true)
		row.add_props("vehicle", 120.0, true)
		if arterial:
			row.add_props("tree", 70.0, true)

	# north-south collectors (columns)
	for c in range(cols + 1):
		var col = RoadCurve.new("St " + str(c + 1))
		for r in range(rows + 1):
			var p = RoadPoint.new()
			p.position = Vector3(c * step, 0, r * step)
			p.bearing = deg_to_rad(90.0)
			p.prior_mag = 38.0
			p.next_mag = 38.0
			p.lanes = 2
			if c == 2:
				p.pathway_l = 2.0
				p.pathway_r = 2.0
			col.add_point(p)
		col.build_mesh()
		col.add_props("tree", 48.0, true)

	print("City grid: " + str(rows + 1 + cols + 1) + " roads — every crossing auto-detected")
