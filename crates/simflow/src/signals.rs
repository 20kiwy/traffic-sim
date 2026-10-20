//! Signal interface: the engine only knows a closure — junction index,
//! approach group and time in, light out. roadkit bridges its
//! [`SignalRegistry`] presets into this.

/// Signal head state for one approach group.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Light {
    Green,
    Amber,
    Red,
}

/// `junction_index × approach_group × time → light`.
pub type SignalFn = Box<dyn Fn(usize, bool, f64) -> Light + Send>;

/// Permanently green (tests / uncontrolled junctions).
pub fn always_green() -> SignalFn {
    Box::new(|_, _, _| Light::Green)
}
