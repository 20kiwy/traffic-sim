# Scenario: highway with interchange — a one-way 3-lane motorway pair
# (dual carriageway with a median), an on-ramp merging in and an off-ramp
# peeling out, speed-limit + no-entry signage, milestones along the verge.

extends Node3D

func _ready():
	print("RoadKit: building highway interchange...")
	auto_junction(true)
	auto_roundabout(false)

	# main carriageway eastbound (one-way, 3 lanes)
	var eb = RoadCurve.new("Hwy E")
	for i in range(5):
		var p = RoadPoint.new()
		p.position = Vector3(-240.0 + i * 120.0, 0, -20.0)
		p.bearing = 0.0
		p.prior_mag = 55.0
		p.next_mag = 55.0
		p.lanes = 3
		p.lane_width = 3.7
		p.oneway = true
		p.median = 2.5
		eb.add_point(p)
	eb.build_mesh()
	eb.add_props("milestone", 150.0, false)

	# main carriageway westbound
	var wb = RoadCurve.new("Hwy W")
	for i in range(5):
		var p = RoadPoint.new()
		p.position = Vector3(240.0 - i * 120.0, 0, 20.0)
		p.bearing = 3.14159265
		p.prior_mag = 55.0
		p.next_mag = 55.0
		p.lanes = 3
		p.lane_width = 3.7
		p.oneway = true
		wb.add_point(p)
	wb.build_mesh()

	# on-ramp: joins Hwy E from the south (T-merge)
	var on = RoadCurve.new("On-Ramp")
	var oa = RoadPoint.new()
	oa.position = Vector3(-160.0, 0, 160.0)
	oa.bearing = -1.15
	oa.lanes = 1
	oa.lane_width = 4.0
	oa.oneway = true
	oa.next_mag = 55.0
	on.add_point(oa)
	var ob = RoadPoint.new()
	ob.position = Vector3(-90.0, 0, 40.0)
	ob.bearing = -0.35
	ob.lanes = 1
	ob.lane_width = 4.0
	ob.oneway = true
	ob.prior_mag = 60.0
	on.add_point(ob)
	on.build_mesh()
	on.stamp_prop("sign_speed_80", 0.35, 5.0)

	# off-ramp: leaves Hwy E toward the north service area
	var off = RoadCurve.new("Off-Ramp")
	var fa = RoadPoint.new()
	fa.position = Vector3(90.0, 0, -20.0)
	fa.bearing = 0.25
	fa.lanes = 1
	fa.lane_width = 4.0
	fa.oneway = true
	fa.next_mag = 55.0
	off.add_point(fa)
	var fb = RoadPoint.new()
	fb.position = Vector3(170.0, 0, -140.0)
	fb.bearing = 1.05
	fb.lanes = 1
	fb.lane_width = 4.0
	fb.oneway = true
	fb.prior_mag = 60.0
	off.add_point(fb)
	off.build_mesh()
	off.stamp_prop("sign_speed_50", 0.5, 5.0)
	off.stamp_prop("sign_warning", 0.15, -5.0)

	# service road ending at the ramp foot (stop-controlled)
	var svc = RoadCurve.new("Service Rd")
	var va = RoadPoint.new()
	va.position = Vector3(120.0, 0, -240.0)
	va.bearing = 1.35
	va.lanes = 2
	va.next_mag = 40.0
	svc.add_point(va)
	var vb = RoadPoint.new()
	vb.position = Vector3(170.0, 0, -140.0)
	vb.bearing = 1.35
	vb.lanes = 2
	vb.prior_mag = 40.0
	svc.add_point(vb)
	svc.build_mesh()
	svc.stamp_prop("sign_stop", 0.85, 6.0)
	svc.stamp_prop("sign_no_entry", 0.1, -6.0)

	print("Highway interchange: 5 roads, ramp merges + service stop")
