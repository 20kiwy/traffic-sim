//! Game time — copied from egregoria `prototypes/src/types/time.rs` (MIT).
//! Fixed 20 ms ticks (50 tps), 10 game-seconds per real second at warp 1.

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct GameTime {
    /// whole ticks since world creation
    pub tick: u64,
    /// seconds since world creation (game seconds)
    pub timestamp: f64,
}

impl GameTime {
    /// seconds per tick
    pub const PERIOD: f64 = 0.02;
    /// seconds in a day
    pub const DAY: f64 = 86400.0;
    /// seconds in an hour
    pub const HOUR: f64 = 3600.0;

    pub fn new() -> Self {
        GameTime {
            tick: 0,
            // day 1 starts at 8:00 like egregoria
            timestamp: 8.0 * Self::HOUR,
        }
    }

    pub fn advance(&mut self) {
        self.tick += 1;
        self.timestamp += Self::PERIOD;
    }

    /// time of day in seconds
    pub fn time_of_day(&self) -> f64 {
        self.timestamp.rem_euclid(Self::DAY)
    }

    /// hours 0..24 for HUD display
    pub fn hours(&self) -> f64 {
        self.time_of_day() / Self::HOUR
    }

    /// "HH:MM" for the HUD
    pub fn hhmm(&self) -> String {
        let h = self.hours();
        let hh = h.floor() as u32;
        let mm = ((h - h.floor()) * 60.0).floor() as u32;
        format!("{:02}:{:02}", hh, mm)
    }
}

impl Default for GameTime {
    fn default() -> Self {
        Self::new()
    }
}
