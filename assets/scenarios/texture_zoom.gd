# Scenario: texture zoom — short, wide cross-sections packed close together
# so the 2D plan (zoomed via `xtask screenshots --zoom`) shows every lane
# marking of the GRG trim sheet: TWO_WAY double-yellow, SLOW white edges,
# MIDDLE dashes, cement BRT median with tactile edges, a dirt road and a
# roundabout island.

extends Node3D

func _ready():
	print("RoadKit: texture zoom lineup...")
	auto_junction(true)
	auto_roundabout(true)

	# 4-lane boulevard with BRT cement median + sidewalks
	var brt = RoadCurve.new("BRT Bd")
	var a1 = RoadPoint.new()
	a1.position = Vector3(-70.0, 0, -22.0)
	a1.bearing = 0.0
	a1.lanes = 4
	a1.median = 3.2
	a1.pathway_l = 2.5
	a1.pathway_r = 2.5
	a1.next_mag = 25.0
	brt.add_point(a1)
	var a2 = RoadPoint.new()
	a2.position = Vector3(70.0, 0, -22.0)
	a2.bearing = 0.0
	a2.lanes = 4
	a2.median = 3.2
	a2.pathway_l = 2.5
	a2.pathway_r = 2.5
	a2.prior_mag = 25.0
	brt.add_point(a2)
	brt.build_mesh()
	brt.stamp_prop("brtStation", 0.5, 0.0)

	# classic two-way with painted lines
	var tw = RoadCurve.new("Two Way")
	var b1 = RoadPoint.new()
	b1.position = Vector3(-70.0, 0, 6.0)
	b1.bearing = 0.0
	b1.lanes = 2
	b1.next_mag = 25.0
	tw.add_point(b1)
	var b2 = RoadPoint.new()
	b2.position = Vector3(70.0, 0, 6.0)
	b2.bearing = 0.0
	b2.lanes = 2
	b2.prior_mag = 25.0
	tw.add_point(b2)
	tw.build_mesh()

	# dirt track: mud lanes + rough shoulders, no markings
	var dirt = RoadCurve.new("Dirt Trk")
	var c1 = RoadPoint.new()
	c1.position = Vector3(-70.0, 0, 26.0)
	c1.bearing = 0.0
	c1.lanes = 2
	c1.lane_width = 3.0
	c1.dirt = true
	c1.shoulder_l = 1.0
	c1.shoulder_r = 1.0
	c1.next_mag = 25.0
	dirt.add_point(c1)
	var c2 = RoadPoint.new()
	c2.position = Vector3(70.0, 0, 26.0)
	c2.bearing = 0.0
	c2.lanes = 2
	c2.lane_width = 3.0
	c2.dirt = true
	c2.shoulder_l = 1.0
	c2.shoulder_r = 1.0
	c2.prior_mag = 25.0
	dirt.add_point(c2)
	dirt.build_mesh()

	# north-south connectors: one crossing both, one ending on Two Way (T)
	var ns = RoadCurve.new("Cross Rd")
	var d1 = RoadPoint.new()
	d1.position = Vector3(-30.0, 0, 60.0)
	d1.bearing = 1.5707963
	d1.lanes = 2
	d1.next_mag = 25.0
	ns.add_point(d1)
	var d2 = RoadPoint.new()
	d2.position = Vector3(-30.0, 0, -60.0)
	d2.bearing = 1.5707963
	d2.lanes = 2
	d2.prior_mag = 25.0
	ns.add_point(d2)
	ns.build_mesh()
	ns.stamp_prop("traffic_light_tz", 0.5, 7.0)

	var lane = RoadCurve.new("Lane")
	var e1 = RoadPoint.new()
	e1.position = Vector3(30.0, 0, 45.0)
	e1.bearing = 1.5707963
	e1.lanes = 1
	e1.next_mag = 20.0
	lane.add_point(e1)
	var e2 = RoadPoint.new()
	e2.position = Vector3(30.0, 0, 6.0)
	e2.bearing = 1.5707963
	e2.lanes = 1
	e2.prior_mag = 20.0
	lane.add_point(e2)
	lane.build_mesh()
	lane.stamp_prop("sign_stop", 0.15, 4.0)

	print("Texture zoom: 5 roads ready — zoom the 2D plan to read the markings")
