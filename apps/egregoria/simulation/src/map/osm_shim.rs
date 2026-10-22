//! Public OSM import API for the external `maptools` crate.
//!
//! `maptools` (the OSM → cartography import library) lives outside the
//! `simulation` crate, so it cannot reach the `pub(crate)` road-building
//! internals. These shims expose exactly the same primitives that
//! `procgen::presets::load_parismap` uses — `add_intersection` + `connect`
//! + a bulk finalizer — under a stable public surface.
//!
//! Nothing about OSM is hardcoded in the app itself: all parsing,
//! classification, projection and bridge engineering lives in `maptools`.

use crate::map::{IntersectionID, LanePattern, Map, RoadID, RoadSegmentKind};
use geom::Vec3;

impl Map {
    /// Adds an intersection at `pos` (OSM import API).
    ///
    /// The caller chooses the z: ground roads use `ROAD_Z_OFFSET`, elevated
    /// bridge intersections pass their deck z.
    pub fn osm_add_intersection(&mut self, pos: Vec3) -> IntersectionID {
        self.add_intersection(pos)
    }

    /// Connects two intersections with a road (OSM import API).
    ///
    /// Mirrors `load_parismap`: roads are connected in bulk without per-road
    /// interface invalidation; call [`Map::osm_finish`] once at the end.
    pub fn osm_connect(
        &mut self,
        src: IntersectionID,
        dst: IntersectionID,
        pattern: &LanePattern,
        segment: RoadSegmentKind,
    ) -> Option<RoadID> {
        self.connect(src, dst, pattern, segment)
    }

    /// Finalizes a bulk OSM import (OSM import API).
    ///
    /// Removes intersections that ended up with no roads (closed loops and
    /// deduped duplicate ways can leave orphans) and checks the map
    /// invariants. The render-side subscriber dispatch is done by the caller
    /// (`Simulation`'s `dispatch_all` at game start).
    pub fn osm_finish(&mut self) {
        let orphans: Vec<IntersectionID> = self
            .intersections
            .iter()
            .filter(|(_, inter)| inter.roads.is_empty())
            .map(|(id, _)| id)
            .collect();
        for id in orphans {
            self.remove_intersection(id);
        }
        self.check_invariants();
    }
}
