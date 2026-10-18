extends Node3D

func _ready():
	var r = RoadCurve.new("Mandela Taper")
	var p0 = RoadPoint.new()
	p0.position = Vector3(-150, 0, 0)
	p0.auto_bearing = true
	p0.lanes = 4
	r.add_point(p0)
	var p1 = RoadPoint.new()
	p1.position = Vector3(-50, 0, 0)
	p1.auto_bearing = true
	p1.lanes = 4
	r.add_point(p1)
	var p2 = RoadPoint.new()
	p2.position = Vector3(50, 0, 0)
	p2.auto_bearing = true
	p2.lanes = 2
	r.add_point(p2)
	var p3 = RoadPoint.new()
	p3.position = Vector3(150, 0, 0)
	p3.auto_bearing = true
	p3.lanes = 2
	r.add_point(p3)
	r.build_mesh()
	print("taper: 4 lanes west, 2 lanes east — wedge between x=-50 and x=50")
