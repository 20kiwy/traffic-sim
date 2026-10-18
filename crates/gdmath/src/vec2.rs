//! Minimal f64 2D vector (used for curve math in the xz plane).

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec2 {
    pub x: f64,
    pub y: f64,
}

impl Vec2 {
    pub const ZERO: Vec2 = Vec2 { x: 0.0, y: 0.0 };

    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    pub fn hypot(&self) -> f64 {
        self.x.hypot(self.y)
    }

    pub fn dist(&self, o: Vec2) -> f64 {
        (self.x - o.x).hypot(self.y - o.y)
    }

    pub fn dot(&self, o: Vec2) -> f64 {
        self.x * o.x + self.y * o.y
    }

    pub fn cross(&self, o: Vec2) -> f64 {
        self.x * o.y - self.y * o.x
    }

    pub fn normalized(&self) -> Vec2 {
        let l = self.hypot();
        if l > 1e-15 {
            Vec2::new(self.x / l, self.y / l)
        } else {
            Vec2::ZERO
        }
    }

    pub fn lerp(&self, o: Vec2, t: f64) -> Vec2 {
        Vec2::new(self.x + (o.x - self.x) * t, self.y + (o.y - self.y) * t)
    }
}

impl std::ops::Add for Vec2 {
    type Output = Vec2;
    fn add(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x + o.x, self.y + o.y)
    }
}

impl std::ops::Sub for Vec2 {
    type Output = Vec2;
    fn sub(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x - o.x, self.y - o.y)
    }
}

impl std::ops::Mul<f64> for Vec2 {
    type Output = Vec2;
    fn mul(self, s: f64) -> Vec2 {
        Vec2::new(self.x * s, self.y * s)
    }
}

impl std::ops::AddAssign for Vec2 {
    fn add_assign(&mut self, o: Vec2) {
        self.x += o.x;
        self.y += o.y;
    }
}
