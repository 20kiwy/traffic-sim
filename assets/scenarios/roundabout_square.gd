# Scenario: roundabout square — two roundabouts on a ring boulevard,
# radials feeding them, a T-junction side road. European-plaza style with
# signal heads and pedestrian signs at the crossing ends.

extends Node3D

func radial(name, cx, cz, ang_deg, r_in, r_out, ln):
        # a straight road pointing away from (cx, cz); ends NEAR the ring
        var curve = RoadCurve.new(name)
        var a1 = ang_deg * 3.14159265 / 180.0
        var x1 = cx + cos(a1) * r_in
        var z1 = cz + sin(a1) * r_in
        var x2 = cx + cos(a1) * r_out
        var z2 = cz + sin(a1) * r_out
        var a = RoadPoint.new()
        a.position = Vector3(x1, 0, z1)
        a.bearing = a1
        a.lanes = ln
        a.next_mag = (r_out - r_in) / 3.0
        curve.add_point(a)
        var b = RoadPoint.new()
        b.position = Vector3(x2, 0, z2)
        b.bearing = a1
        b.lanes = ln
        b.prior_mag = (r_out - r_in) / 3.0
        curve.add_point(b)
        curve.build_mesh()
        return curve

func _ready():
        print("RoadKit: building roundabout square...")
        auto_junction(true)
        auto_roundabout(true)
        set_island_radius(8.0)

        # dressing: signals + signs + lights around the plazas
        var pn = radial("Plaza N", 0.0, 0.0, 270.0, 3.0, 150.0, 3)
        pn.stamp_prop("traffic_light_tz", 0.5, 8.0)
        pn.stamp_prop("sign_road_name", 0.2, 8.0)
        pn.stamp_prop("sign_pedestrian", 0.85, -8.0)
        var pw = radial("Plaza W", 0.0, 0.0, 150.0, 3.0, 150.0, 3)
        pw.stamp_prop("traffic_light_eu", 0.4, -8.0)
        pw.stamp_prop("sign_speed_50", 0.8, 8.0)
        var ps = radial("Plaza S", 0.0, 0.0, 30.0, 3.0, 150.0, 3)
        ps.stamp_prop("sign_warning", 0.9, -7.0)
        var es = radial("East S", 320.0, 40.0, 80.0, 3.0, 170.0, 2)
        es.stamp_prop("sign_stop", 0.15, -8.0)
        es.stamp_prop("streetlight", 0.5, 9.0)

        # second roundabout to the east: 4 legs -> bigger ring
        radial("East N", 320.0, 40.0, 280.0, 3.0, 150.0, 3)
        radial("East W", 320.0, 40.0, 185.0, 3.0, 150.0, 3)
        radial("East E", 320.0, 40.0, 0.0, 3.0, 150.0, 3)

        # a T-junction: side road ending on Plaza S
        var side = RoadCurve.new("Market Ln")
        var sa = RoadPoint.new()
        sa.position = Vector3(80.0, 0, 190.0)
        sa.bearing = -0.9
        sa.lanes = 2
        sa.next_mag = 30.0
        side.add_point(sa)
        var sb = RoadPoint.new()
        sb.position = Vector3(120.0, 0, 120.0)
        sb.bearing = -0.9
        sb.lanes = 2
        sb.prior_mag = 30.0
        side.add_point(sb)
        side.build_mesh()
        side.stamp_prop("sign_yield", 0.8, 6.0)

        print("Roundabout square: 8 roads — junctions:")
        print(junction_info())
