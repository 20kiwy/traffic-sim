# RoadKit Godot — BRT corridor
# A Mwendokasi-style bus rapid transit trunk: 4 lanes with a 3.2 m cement
# median, median stations (30 m canopy platform + small shelter) stamped
# into the median, speed limits stepping down toward the stops, no-entry
# plates protecting the contraflow side, and streetlights along the verge.
# Assets: brtStation / brtShelterSmall / sign* from assets/glb/manifest.json.

extends Node3D

func _ready():
        print("RoadKit: building BRT corridor...")

        var corridor = RoadCurve.new("BRT Trunk 1")
        for i in range(6):
                var p = RoadPoint.new()
                p.position = Vector3(-250 + i * 100, 0, i * 18)
                p.auto_bearing = true
                p.prior_mag = 60.0
                p.next_mag = 60.0
                p.lanes = 4
                p.median = 3.2
                corridor.add_point(p)
        corridor.build_mesh()
        print("BRT Trunk 1 length: " + str(corridor.length()) + " m")

        # Median stations between the carriageways (lateral 0 = centerline),
        # the 30 m flagship midway and the 12 m shelter at the far end.
        corridor.stamp_prop("brtStation", 0.30, 0.0)
        corridor.stamp_prop("brtStation", 0.62, 0.0)
        corridor.stamp_prop("brtShelterSmall", 0.92, 0.0)

        # Signage: 80 approaching, 50 through the station zone, no-entry on
        # the wrong side of the median, a warning triangle before the stop.
        corridor.stamp_prop("signSpeed80", 0.05, 5.5)
        corridor.stamp_prop("signSpeed50", 0.24, 5.5)
        corridor.stamp_prop("signWarning", 0.55, 5.5)
        corridor.stamp_prop("signNoEntry", 0.62, -5.5)
        corridor.stamp_prop("signSpeed80", 0.95, 5.5)

        # Pedestrian crossings at the stations + verge lighting.
        corridor.stamp_prop("signPedestrian", 0.34, 6.0)
        corridor.stamp_prop("signPedestrian", 0.66, 6.0)
        corridor.add_props("streetlight", 45.0, true)

        print("RoadKit: BRT corridor dressed — see the 3D Scene tab")
