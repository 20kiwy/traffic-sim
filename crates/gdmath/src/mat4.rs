//! Row-major 4x4 matrices (column-vector convention: p' = M * p) and the
//! orbit camera for the software 3D viewport. No external dependencies.

use crate::vec3::Vec3;

#[derive(Clone, Copy, Debug)]
pub struct Mat4 {
    /// row-major: m[row * 4 + col]
    pub m: [f32; 16],
}

impl Mat4 {
    pub const IDENTITY: Mat4 = Mat4 {
        m: [
            1.0, 0.0, 0.0, 0.0, //
            0.0, 1.0, 0.0, 0.0, //
            0.0, 0.0, 1.0, 0.0, //
            0.0, 0.0, 0.0, 1.0,
        ],
    };

    pub fn translation(x: f32, y: f32, z: f32) -> Mat4 {
        let mut m = Self::IDENTITY;
        m.m[3] = x;
        m.m[7] = y;
        m.m[11] = z;
        m
    }

    pub fn scale(sx: f32, sy: f32, sz: f32) -> Mat4 {
        let mut m = Self::IDENTITY;
        m.m[0] = sx;
        m.m[5] = sy;
        m.m[10] = sz;
        m
    }

    /// Rotation about the Y axis (right-handed, counterclockwise from above).
    pub fn rot_y(theta: f32) -> Mat4 {
        let (s, c) = theta.sin_cos();
        Mat4 {
            m: [
                c, 0.0, s, 0.0, //
                0.0, 1.0, 0.0, 0.0, //
                -s, 0.0, c, 0.0, //
                0.0, 0.0, 0.0, 1.0,
            ],
        }
    }

    /// Right-handed perspective projection.
    pub fn perspective(fov_y_radians: f32, aspect: f32, near: f32, far: f32) -> Mat4 {
        let f = 1.0 / (fov_y_radians / 2.0).tan();
        let nf = 1.0 / (near - far);
        Mat4 {
            m: [
                f / aspect, 0.0, 0.0, 0.0, //
                0.0, f, 0.0, 0.0, //
                0.0, 0.0, (far + near) * nf, 2.0 * far * near * nf, //
                0.0, 0.0, -1.0, 0.0,
            ],
        }
    }

    /// Right-handed look-at view matrix (camera looks down -Z).
    pub fn look_at(eye: Vec3, center: Vec3, up: Vec3) -> Mat4 {
        let f = (center - eye).normalized(); // forward
        let s = f.cross(up).normalized(); // right
        let u = s.cross(f); // true up
        Mat4 {
            m: [
                s.x, s.y, s.z, -s.dot(eye), //
                u.x, u.y, u.z, -u.dot(eye), //
                -f.x, -f.y, -f.z, f.dot(eye), //
                0.0, 0.0, 0.0, 1.0,
            ],
        }
    }

    /// Matrix product self * other (other applied first).
    pub fn mul(&self, o: &Mat4) -> Mat4 {
        let mut out = [0f32; 16];
        for r in 0..4 {
            for c in 0..4 {
                let mut acc = 0.0;
                for k in 0..4 {
                    acc += self.m[r * 4 + k] * o.m[k * 4 + c];
                }
                out[r * 4 + c] = acc;
            }
        }
        Mat4 { m: out }
    }

    /// Transform a point, returning position + w (for clipping).
    pub fn transform_point(&self, p: Vec3) -> (Vec3, f32) {
        let x = p.x;
        let y = p.y;
        let z = p.z;
        let ox = self.m[0] * x + self.m[1] * y + self.m[2] * z + self.m[3];
        let oy = self.m[4] * x + self.m[5] * y + self.m[6] * z + self.m[7];
        let oz = self.m[8] * x + self.m[9] * y + self.m[10] * z + self.m[11];
        let w = self.m[12] * x + self.m[13] * y + self.m[14] * z + self.m[15];
        (Vec3::new(ox, oy, oz), w)
    }

    /// Model matrix for a prop instance (translate * rotY * scale).
    pub fn prop_model(pos: Vec3, yaw: f32, scale: f32) -> Mat4 {
        let t = Self::translation(pos.x, pos.y, pos.z);
        let r = Self::rot_y(yaw);
        let s = Self::scale(scale, scale, scale);
        t.mul(&r).mul(&s)
    }
}

/// Orbit camera (target + spherical offset).
#[derive(Clone, Copy, Debug)]
pub struct OrbitCam {
    pub target: Vec3,
    pub dist: f32,
    /// radians
    pub yaw: f32,
    /// radians above the horizon
    pub pitch: f32,
}

impl Default for OrbitCam {
    fn default() -> Self {
        Self {
            target: Vec3::ZERO,
            dist: 160.0,
            yaw: 0.65,
            pitch: 0.5,
        }
    }
}

impl OrbitCam {
    pub fn eye(&self) -> Vec3 {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        Vec3::new(
            self.target.x + self.dist * cp * cy,
            self.target.y + self.dist * sp,
            self.target.z + self.dist * cp * sy,
        )
    }

    pub fn view(&self) -> Mat4 {
        Mat4::look_at(self.eye(), self.target, Vec3::new(0.0, 1.0, 0.0))
    }

    pub fn proj(&self, aspect: f32) -> Mat4 {
        Mat4::perspective(0.9, aspect, 0.1, 100_000.0)
    }
}
