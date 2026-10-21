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

    /// Right-handed orthographic projection (camera looks down -Z).
    /// Used by the sun shadow map (an ortho fit of the scene in light space).
    pub fn ortho(left: f32, right: f32, bottom: f32, top: f32, near: f32, far: f32) -> Mat4 {
        let sx = 2.0 / (right - left);
        let sy = 2.0 / (top - bottom);
        let sz = -2.0 / (far - near);
        let tx = -(right + left) / (right - left);
        let ty = -(top + bottom) / (top - bottom);
        let tz = -(far + near) / (far - near);
        Mat4 {
            m: [
                sx, 0.0, 0.0, tx, //
                0.0, sy, 0.0, ty, //
                0.0, 0.0, sz, tz, //
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
///
/// R15 adds the Egregoria `OrbitCamera` feel (native_app/src/rendering/
/// orbit_camera.rs, MIT): every input moves a TARGET pose and the real pose
/// follows with exponential damping — `lerpp!` amounts pos 8 / yaw 16 /
/// pitch 8 / dist 8, snap threshold 0.002, wheel zoom 1.05^(0.5+0.1|w|),
/// dist clamp [5, maxdist]. `dist`/`yaw`/`pitch` stay the authoritative
/// render state; the `target_*` fields only exist so `update()` can ease
/// toward them (headless renderers that never call `update()` are unaffected).
#[derive(Clone, Copy, Debug)]
pub struct OrbitCam {
    pub target: Vec3,
    pub dist: f32,
    /// radians
    pub yaw: f32,
    /// radians above the horizon
    pub pitch: f32,
    /// pending smooth-zoom velocity (Google-Maps wheel feel; consumed
    /// and decayed by the app's update loop)
    pub zoom_vel: f32,
    /// vertical FOV in radians (egregoria default 60° = 1.0472; legacy
    /// default stays 0.9 so existing renders don't shift)
    pub fov: f32,
    /// damped-follow targets (Egregoria camera_movement model)
    pub target_yaw: f32,
    pub target_pitch: f32,
    pub target_dist: f32,
}

impl Default for OrbitCam {
    fn default() -> Self {
        Self {
            target: Vec3::ZERO,
            dist: 160.0,
            yaw: 0.65,
            pitch: 0.5,
            zoom_vel: 0.0,
            fov: 0.9,
            target_yaw: 0.65,
            target_pitch: 0.5,
            target_dist: 160.0,
        }
    }
}

impl OrbitCam {
    /// Exponential-damping follow step (Egregoria `lerpp!` port):
    /// `coeff = dt * tightness * amount`; snap when overshoot or the diff
    /// is below the threshold. `amounts`: pos 8, yaw 16, pitch 8, dist 8.
    pub fn update(&mut self, dt: f32) {
        let dt = dt.min(0.1); // Egregoria clamps the frame delta the same way
        // yaw (16)
        let coeff = dt * 16.0;
        let diff = self.target_yaw - self.yaw;
        if coeff > 1.0 || diff.abs() < 0.002 {
            self.yaw = self.target_yaw;
        } else {
            self.yaw += diff * coeff;
        }
        // pitch (8)
        let coeff = dt * 8.0;
        let diff = self.target_pitch - self.pitch;
        if coeff > 1.0 || diff.abs() < 0.002 {
            self.pitch = self.target_pitch;
        } else {
            self.pitch += diff * coeff;
        }
        // dist (8)
        let coeff = dt * 8.0;
        let diff = self.target_dist - self.dist;
        if coeff > 1.0 || diff.abs() < 0.002 {
            self.dist = self.target_dist;
        } else {
            self.dist += diff * coeff;
        }
        // target pos rides along with the eased dist/yaw/pitch (the orbit
        // target itself is set directly by pans, like Egregoria's targetpos)
        self.sync_targets();
    }

    /// Re-arm the follow targets at the current pose (after programmatic
    /// moves — fit_scene, presets, CLI framing) so the camera doesn't drift.
    pub fn sync_targets(&mut self) {
        self.target_yaw = self.yaw;
        self.target_pitch = self.pitch;
        self.target_dist = self.dist;
    }

    /// Egregoria wheel zoom: `targetdist *= (1/1.05)^(0.5 + 0.1*|wheel|)`
    /// zooming in, `* 1.05^(...)` zooming out.
    pub fn zoom_wheel(&mut self, wheel: f32) {
        let k = 0.5 + 0.1 * wheel.abs();
        if wheel > 0.0 {
            self.target_dist *= (1.0 / 1.05f32).powf(k);
        } else if wheel < 0.0 {
            self.target_dist *= 1.05f32.powf(k);
        }
        self.target_dist = self.target_dist.clamp(5.0, 1500.0);
    }

    pub fn eye(&self) -> Vec3 {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        Vec3::new(
            self.target.x + self.dist * cp * cy,
            self.target.y + self.dist * sp,
            self.target.z + self.dist * cp * sy,
        )
    }

    /// Normalized view direction (from the eye toward the orbit target).
    pub fn view_dir(&self) -> Vec3 {
        (self.target - self.eye()).normalized()
    }

    pub fn view(&self) -> Mat4 {
        Mat4::look_at(self.eye(), self.target, Vec3::new(0.0, 1.0, 0.0))
    }

    pub fn proj(&self, aspect: f32) -> Mat4 {
        Mat4::perspective(self.fov, aspect, 0.1, 100_000.0)
    }
}
