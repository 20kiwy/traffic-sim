use engine::heightmap::HeightmapRender;
use engine::{Context, FrameContext, GfxContext};
use geom::Camera;
use simulation::map::{Map, MapSubscriber, UpdateType};
use simulation::Simulation;

const CSIZE: u32 = simulation::map::Heightmap::SIZE;
const CRESO: usize = simulation::map::Heightmap::RESOLUTION;

pub struct TerrainRender {
    /// None when the map has no terrain chunks (OSM carto build: flat
    /// ground is drawn by the cartography renderer instead).
    heightmap: Option<HeightmapRender<CSIZE, CRESO>>,
    terrain_sub: MapSubscriber,
}

impl TerrainRender {
    pub fn new(gfx: &mut GfxContext, sim: &Simulation) -> Self {
        let (w, h) = sim.map().environment.size();

        // A 0x0 heightmap would create a zero-sized texture — skip it.
        let terrain = if w == 0 || h == 0 {
            None
        } else {
            Some(HeightmapRender::new(gfx, w as u32, h as u32))
        };

        Self {
            heightmap: terrain,
            terrain_sub: sim.map().subscribe(UpdateType::Terrain),
        }
    }

    pub fn draw(&mut self, cam: &Camera, fctx: &mut FrameContext<'_>) {
        if let Some(heightmap) = &mut self.heightmap {
            heightmap.draw_heightmap(cam, fctx);
        }
    }

    pub fn update(&mut self, ctx: &mut Context, map: &Map) {
        let Some(heightmap) = &mut self.heightmap else {
            return;
        };
        let ter = &map.environment;

        if self.terrain_sub.take_cleared() {
            for (chunk_id, chunk) in ter.chunks() {
                heightmap.update_chunk(
                    &mut ctx.gfx,
                    (chunk_id.0 as u32, chunk_id.1 as u32),
                    chunk,
                );
            }

            heightmap.invalidate_height_normals(&ctx.gfx);
            return;
        }

        let mut changed = false;
        for cell in self.terrain_sub.take_updated_chunks() {
            for chunkid in cell.convert() {
                let chunk =
                    unwrap_retlog!(ter.get_chunk(chunkid), "trying to update nonexistent chunk");

                heightmap.update_chunk(
                    &mut ctx.gfx,
                    (chunkid.0 as u32, chunkid.1 as u32),
                    chunk,
                );
            }
            changed = true;
        }

        if changed {
            heightmap.invalidate_height_normals(&ctx.gfx);
        }
    }
}
