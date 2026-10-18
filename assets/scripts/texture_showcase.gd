# RoadKit Godot — texture showcase
# One 4-lane boulevard showing the FULL Godot Road Generator trim sheet:
# SLOW (white edge) + MIDDLE (dashed) + FAST/TWO_WAY (double-yellow center),
# a concrete median, sidewalks both sides, shoulders — plus a crossing
# side street so the n-gon junction patch blends with the texture.

extends Node3D

func road(name, x1, z1, x2, z2, ln, med, path, shldr):
	var curve = RoadCurve.new(name)
	var a = RoadPoint.new()
	a.position = Vector3(x1, 0, z1)
	a.bearing = atan2(z2 - z1, x2 - x1)
	a.lanes = ln
	a.lane_width = 3.5
	a.median = med
	a.pathway_l = path
	a.pathway_r = path
	a.shoulder_l = shldr
	a.shoulder_r = shldr
	a.next_mag = 40.0
	curve.add_point(a)
	var b = RoadPoint.new()
	b.position = Vector3(x2, 0, z2)
	b.bearing = atan2(z2 - z1, x2 - x1)
	b.lanes = ln
	b.lane_width = 3.5
	b.median = med
	b.pathway_l = path
	b.pathway_r = path
	b.shoulder_l = shldr
	b.shoulder_r = shldr
	b.prior_mag = 40.0
	curve.add_point(b)
	curve.build_mesh()
	return curve

func _ready():
	print("RoadKit: texture showcase — 4-lane boulevard + crossing...")
	auto_junction(true)
	auto_roundabout(false)

	# main boulevard, east-west
	road("Boulevard", -90, 0, 90, 0, 4, 2.0, 3.0, 1.0)
	# crossing street, north-south
	road("Cross St", 0, -70, 0, 70, 2, 0.0, 2.0, 0.0)

	curve_props()

func curve_props():
	print("texture showcase built — check the trim-sheet markings up close")
