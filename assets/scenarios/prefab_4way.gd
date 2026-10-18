extends Node3D

func _ready():
	var ew = RoadCurve.new("EW")
	for x in [-160, 0, 160]:
		var p = RoadPoint.new()
		p.position = Vector3(x, 0, 0)
		p.auto_bearing = true
		p.lanes = 2
		ew.add_point(p)
	ew.build_mesh()
	var ns = RoadCurve.new("NS")
	for z in [-160, 0, 160]:
		var p = RoadPoint.new()
		p.position = Vector3(0, 0, z)
		p.auto_bearing = true
		p.lanes = 2
		ns.add_point(p)
	ns.build_mesh()
	junction_prefabs(true)
	print(junction_info())
