//! Intelligent Driver Model (Treiber/Kesting) — the longitudinal
//! control law behind simflow: smooth acceleration toward the desired
//! speed, comfortable braking for leaders, red lights and pedestrians.

/// IDM parameter set (urban calibration).
#[derive(Clone, Copy, Debug)]
pub struct Idm {
    /// maximum acceleration (m/s²)
    pub a: f64,
    /// comfortable deceleration (m/s²)
    pub b: f64,
    /// desired time headway (s)
    pub t_headway: f64,
    /// minimum standstill gap (m)
    pub s0: f64,
    /// acceleration exponent
    pub delta: f64,
}

impl Default for Idm {
    fn default() -> Self {
        Self { a: 2.0, b: 3.0, t_headway: 1.1, s0: 2.2, delta: 4.0 }
    }
}

impl Idm {
    /// Acceleration for one vehicle: `v` speed, `v0` desired speed,
    /// `gap` bumper-to-leader distance (f64::MAX = free road),
    /// `dv` approach speed (v - v_leader; ignored when gap infinite).
    pub fn accel(&self, v: f64, v0: f64, gap: f64, dv: f64) -> f64 {
        let v0 = v0.max(0.1);
        let free = 1.0 - (v / v0).powf(self.delta);
        if !gap.is_finite() {
            return self.a * free.max(0.0);
        }
        let gap = gap.max(0.1);
        let s_star = self.s0 + (v * self.t_headway + v * dv / (2.0 * (self.a * self.b).sqrt())).max(0.0);
        let interaction = (s_star / gap).powi(2);
        self.a * (free - interaction)
    }

    /// Whether a vehicle at `v` can still stop comfortably (≤ 3 m/s²)
    /// within `dist` — the amber dilemma-zone test.
    pub fn can_stop_comfortably(&self, v: f64, dist: f64) -> bool {
        v * v / (2.0 * dist.max(0.1)) <= 3.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_road_accelerates_to_desired() {
        let idm = Idm::default();
        assert!(idm.accel(0.0, 14.0, f64::MAX, 0.0) > 1.5, "full acceleration from standstill");
        assert!(idm.accel(14.0, 14.0, f64::MAX, 0.0).abs() < 0.01, "cruise at desired speed");
    }

    #[test]
    fn stalled_leader_brakes_hard() {
        let idm = Idm::default();
        let acc = idm.accel(10.0, 14.0, 4.0, 10.0);
        assert!(acc < -2.0, "hard braking behind a stalled leader ({acc})");
    }

    #[test]
    fn amber_dilemma() {
        let idm = Idm::default();
        assert!(idm.can_stop_comfortably(8.0, 20.0), "slow + far = stop");
        assert!(!idm.can_stop_comfortably(14.0, 3.0), "fast + close = clear");
    }
}
