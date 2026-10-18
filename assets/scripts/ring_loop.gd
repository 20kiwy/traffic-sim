# RoadKit Godot — ring loop
# A closed ring road built with a for-loop: 10 RoadPoints on a circle,
# closed curve, then the GLB prop parade around it.

extends Node3D

func _ready():
	print("RoadKit: building ring loop...")
	var ring = RoadCurve.new("Ring Road")
	ring.closed = true

	var n = 10
	var radius = 90.0
	for i in range(n):
		var ang = deg_to_rad(i * 36.0)
		var p = RoadPoint.new()
		p.position = Vector3(cos(ang) * radius, 0, sin(ang) * radius)
		p.bearing = ang + deg_to_rad(90.0)
		p.prior_mag = 32.0
		p.next_mag = 32.0
		p.lanes = 2
		ring.add_point(p)

	ring.build_mesh()
	print("Ring Road length: " + str(ring.length()) + " m")

	ring.add_props("streetlight", 40.0, true)
	ring.add_props("vehicle", 55.0, true)
	ring.add_props("tree", 22.0, true)
	print("Ring done — props all the way around")
