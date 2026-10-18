# RoadKit Godot — Dar es Salaam signalized crossing (-6.807369, 39.257743)
# Traced from the Esri satellite tile at that point: an E-W dual
# carriageway with a planted median crossed by a slightly skewed N-S
# collector, plus a side street feeding in from the north-west. Dressed
# with TZ signal heads, speed limits, street-name blades and streetlights,
# and it is the traffic-simulation showcase (--sim 20 in roadkit-cli,
# or the "traffic" toggle in the 3D Scene toolbar).

extends Node3D

func _ready():
        print("RoadKit: building Dar es Salaam crossing (-6.807369, 39.257743)...")

        # E-W dual carriageway (3 lanes each way + median), gentle 4-degree drift
        var ew = RoadCurve.new("Mandela Rd")
        for x in [-170, -60, 60, 170]:
                var p = RoadPoint.new()
                p.position = Vector3(x, 0, 14 - x * 0.09)
                p.auto_bearing = true
                p.lanes = 6
                p.lane_width = 3.3
                p.median = 3.0
                p.pathway_l = 3.0
                p.pathway_r = 3.0
                p.shoulder_l = 0.5
                p.shoulder_r = 0.5
                ew.add_point(p)
        ew.build_mesh()

        # N-S collector, 2 lanes each way, slight S-skew like the satellite
        var ns = RoadCurve.new("Chang'ombe St")
        var n1 = RoadPoint.new()
        n1.position = Vector3(18, 0, -170)
        n1.auto_bearing = true
        n1.lanes = 4
        n1.lane_width = 3.2
        n1.median = 1.6
        n1.pathway_l = 2.5
        n1.pathway_r = 2.5
        ns.add_point(n1)
        var n2 = RoadPoint.new()
        n2.position = Vector3(8, 0, -80)
        n2.auto_bearing = true
        n2.lanes = 4
        n2.lane_width = 3.2
        n2.median = 1.6
        n2.pathway_l = 2.5
        n2.pathway_r = 2.5
        ns.add_point(n2)
        var n3 = RoadPoint.new()
        n3.position = Vector3(0, 0, 0)
        n3.auto_bearing = true
        n3.lanes = 4
        n3.lane_width = 3.2
        n3.median = 1.6
        n3.pathway_l = 2.5
        n3.pathway_r = 2.5
        ns.add_point(n3)
        var n4 = RoadPoint.new()
        n4.position = Vector3(-9, 0, 80)
        n4.auto_bearing = true
        n4.lanes = 4
        n4.lane_width = 3.2
        n4.median = 1.6
        n4.pathway_l = 2.5
        n4.pathway_r = 2.5
        ns.add_point(n4)
        var n5 = RoadPoint.new()
        n5.position = Vector3(-16, 0, 170)
        n5.auto_bearing = true
        n5.lanes = 4
        n5.lane_width = 3.2
        n5.median = 1.6
        n5.pathway_l = 2.5
        n5.pathway_r = 2.5
        ns.add_point(n5)
        ns.build_mesh()

        # side street from the north-west (T-join onto the N-S collector)
        var side = RoadCurve.new("Gerezani Link")
        var sa = RoadPoint.new()
        sa.position = Vector3(-110, 0, -95)
        sa.auto_bearing = true
        sa.lanes = 2
        sa.lane_width = 3.0
        side.add_point(sa)
        var sb = RoadPoint.new()
        sb.position = Vector3(6, 0, -52)
        sb.auto_bearing = true
        sb.lanes = 2
        sb.lane_width = 3.0
        side.add_point(sb)
        side.build_mesh()

        # TZ signal heads on the dual carriageway approaches (right-hand pole),
        # EU-style with pedestrian man on the collector
        ew.add_props_offset("trafficLightTz", 300.0, 8.0, false, true)
        ns.add_props_offset("trafficLightEu", 220.0, 6.5, false, true)

        # regulatory + guidance signs around the crossing
        ew.stamp_prop("signSpeed50", 0.32, -8.5)
        ew.stamp_prop("signSpeed50", 0.68, 8.5)
        ew.stamp_prop("signNoEntry", 0.5, -9.0)
        ns.stamp_prop("signStop", 0.03, 5.5)
        ns.stamp_prop("signWarning", 0.97, -5.5)
        side.stamp_prop("signYield", 0.97, 3.5)
        ew.stamp_prop("signRoadName", 0.5, 7.0)
        ns.stamp_prop("signRoadName", 0.55, -6.0)
        ew.stamp_prop("signPedestrian", 0.42, 8.5)

        # street furniture: lights along both roads, trees on the side street
        ew.add_props("streetlight", 55.0, true)
        ns.add_props("streetlight", 70.0, true)
        side.add_props("tree", 60.0, true)
        ew.add_props("milestone", 120.0, true)

        junction_prefabs(true)
        print("RoadKit: Dar crossing ready — run with traffic to see the signals work")
        print(junction_info())
