# RoadKit Godot — signalized junction
# A 4-way crossroads dressed with the generated traffic pack: TZ and EU
# signal heads on the approaches, the US mast-arm signal reaching over the
# road, then regulatory signs (stop, give-way, speed limits, no-entry),
# a warning triangle, a street-name blade and a pedestrian crossing sign.
# Assets come from assets/glb (see manifest.json, ids trafficLight*/sign*).

extends Node3D

func _ready():
        print("RoadKit: building signalized junction...")

        # North-south collector through the origin (auto_bearing: no trig)
        var ns = RoadCurve.new("Kivukoni St")
        for z in [-140, 0, 140]:
                var p = RoadPoint.new()
                p.position = Vector3(0, 0, z)
                p.auto_bearing = true
                p.prior_mag = 70.0
                p.next_mag = 70.0
                p.lanes = 2
                ns.add_point(p)
        ns.build_mesh()

        # East-west arterial through the same point -> auto junction
        var ew = RoadCurve.new("Mwendokasi Rd")
        for x in [-180, 0, 180]:
                var p = RoadPoint.new()
                p.position = Vector3(x, 0, 0)
                p.auto_bearing = true
                p.prior_mag = 90.0
                p.next_mag = 90.0
                p.lanes = 3
                ew.add_point(p)
        ew.build_mesh()

        # Signals on the approaches (poles stand just off the shoulder):
        # TZ head + EU head with pedestrian man on the narrow street,
        # US mast arm hanging its head over the arterial lanes.
        ns.add_props_offset("trafficLightTz", 280.0, 7.5, false, true)
        ns.stamp_prop("trafficLightEu", 0.35, -7.5)
        ew.add_props_offset("trafficLightUsa", 360.0, 8.0, false, true)

        # Regulatory signs facing the approaches
        ew.stamp_prop("signStop", 0.02, 5.0)
        ew.stamp_prop("signYield", 0.98, 5.0)
        ns.stamp_prop("signSpeed50", 0.02, 4.5)
        ns.stamp_prop("signSpeed80", 0.98, 4.5)
        ew.stamp_prop("signNoEntry", 0.30, -5.0)
        ew.stamp_prop("signWarning", 0.70, 5.5)

        # Street furniture: name blade at the corner, crossing sign + bollard
        ns.stamp_prop("signRoadName", 0.50, 4.5)
        ew.stamp_prop("signPedestrian", 0.44, 5.5)

        print("RoadKit: junction signalized — see the 3D Scene tab")
