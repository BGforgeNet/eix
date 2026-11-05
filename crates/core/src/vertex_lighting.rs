//! One vertex's colour as 3DfpFPU.dll's per-vertex lighting computes it (its transform pipeline's method at RVA 0xD910,
//! x87 code), for the engine fix that replaces it (`VertexLighting`, docs/engine-fixes.md): one directional
//! light, the point lights in range, each colour channel the larger of what it had and a light's share, then the
//! ambient light, a cap of 1 and the four channels packed as ARGB bytes.
//!
//! The x87 code rounds each operation to the precision its control word sets and each value it stores as a 32-bit
//! float to single precision. [`Lanes`] carries four vertices at one precision, [`Lanes::single`] stands for the
//! stores, and the operations follow the original's order, so both precisions the game runs under give the
//! original's bits.

use std::array::from_fn;
use std::ops::{Add, Mul, Sub};

use crate::num::round_to_i32;

/// Four vertices' values side by side, as the x87 computes each at one precision setting: 32-bit floats for its
/// 24-bit setting (the control word Direct3D sets for the game's `DDSCL_FPUSETUP`), 64-bit floats for its 53-bit one
/// (the C runtime's default). Each lane's operation is the scalar IEEE one. Values outside the type's exponent range,
/// which the x87's wider exponent would keep, are not reproduced.
pub trait Lanes: Copy + Add<Output = Self> + Sub<Output = Self> + Mul<Output = Self> {
    /// A lane mask of a comparison.
    type Mask: Copy;
    /// A 32-bit float in every lane (`fld dword`): exact.
    fn splat(x: f32) -> Self;
    /// Four 32-bit floats, lane 0 first: exact.
    fn load(x: [f32; 4]) -> Self;
    /// Lane-wise `1 / self`.
    #[must_use]
    fn recip(self) -> Self;
    /// Lane-wise `fsqrt`.
    #[must_use]
    fn sqrt(self) -> Self;
    /// Each lane stored as a 32-bit float and loaded again (`fstp dword`, `fld dword`): rounded to nearest.
    #[must_use]
    fn single(self) -> Self;
    /// Each lane as a 32-bit float, rounded to nearest.
    fn floats(self) -> [f32; 4];
    /// Where `self >= other`: a NaN on either side is not (`fcomp` sets C0 for "less" and for "unordered").
    fn at_least(self, other: Self) -> Self::Mask;
    /// Whether any lane of `mask` is set.
    fn any(mask: Self::Mask) -> bool;
    /// `yes` in the lanes `mask` has, `no` in the others, bit for bit.
    fn select(mask: Self::Mask, yes: Self, no: Self) -> Self;
}

/// [`Lanes`] as four plain floats, `f32` or `f64`, one operation at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scalar<T>(pub [T; 4]);

macro_rules! scalar_lanes {
    ($t:ty, $from:expr, $to:expr) => {
        impl Add for Scalar<$t> {
            type Output = Self;
            fn add(self, o: Self) -> Self {
                Self(from_fn(|i| self.0[i] + o.0[i]))
            }
        }

        impl Sub for Scalar<$t> {
            type Output = Self;
            fn sub(self, o: Self) -> Self {
                Self(from_fn(|i| self.0[i] - o.0[i]))
            }
        }

        impl Mul for Scalar<$t> {
            type Output = Self;
            fn mul(self, o: Self) -> Self {
                Self(from_fn(|i| self.0[i] * o.0[i]))
            }
        }

        impl Lanes for Scalar<$t> {
            type Mask = [bool; 4];
            fn splat(x: f32) -> Self {
                Self([$from(x); 4])
            }
            fn load(x: [f32; 4]) -> Self {
                Self(x.map($from))
            }
            fn recip(self) -> Self {
                Self(self.0.map(|x| 1.0 / x))
            }
            fn sqrt(self) -> Self {
                Self(self.0.map(<$t>::sqrt))
            }
            fn single(self) -> Self {
                Self(self.0.map(|x| $from($to(x))))
            }
            fn floats(self) -> [f32; 4] {
                self.0.map($to)
            }
            fn at_least(self, other: Self) -> [bool; 4] {
                from_fn(|i| self.0[i] >= other.0[i])
            }
            fn any(mask: [bool; 4]) -> bool {
                mask.contains(&true)
            }
            fn select(mask: [bool; 4], yes: Self, no: Self) -> Self {
                Self(from_fn(|i| if mask[i] { yes.0[i] } else { no.0[i] }))
            }
        }
    };
}

scalar_lanes!(f32, |x: f32| x, |x: f32| x);
scalar_lanes!(f64, f64::from, |x: f64| x as f32);

/// The directional light: where it shines from, its colour and the alpha every vertex gets.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Directional {
    /// The direction, against which a vertex's normal is dotted.
    pub direction: [f32; 3],
    /// Red, green, blue.
    pub colour: [f32; 3],
    /// The alpha of every vertex, 0 to 1.
    pub alpha: f32,
}

/// A point light.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointLight {
    /// Where it is.
    pub position: [f32; 3],
    /// How fast it fades: its share is scaled by `1 - falloff * distance^2`, and it reaches no vertex where that is
    /// below 0.
    pub falloff: f32,
    /// Red, green, blue.
    pub colour: [f32; 3],
}

/// The larger of a channel's value and a light's share as the original picks it (`fcomp`, then the sign of C0): the
/// share, unless the value is at least as large; a NaN on either side picks the share.
fn larger<L: Lanes>(value: L, share: L) -> L {
    L::select(value.at_least(share), value, share)
}

/// A channel capped at 1 as the original caps it: 1 when at least 1, else the channel itself (NaN too).
fn capped<L: Lanes>(channel: L) -> L {
    let one = L::splat(1.0);
    L::select(channel.at_least(one), one, channel)
}

/// Channels from 0 to 1 as the bytes the original packs (times 255, stored as a float, then `fistp`). Outside 0 to
/// 255 a value is what the integer conversion gives, which the packing does not mask.
fn bytes<L: Lanes>(channel: L) -> [u32; 4] {
    (channel * L::splat(255.0))
        .floats()
        .map(|x| round_to_i32(f64::from(x)) as u32)
}

/// The colour of one vertex, ARGB: `floor` is the lowest each channel starts from (the pipeline's), `normal` and
/// `position` are the vertex's, `lights` the point lights in the original's order, `ambient` added at the end.
pub fn vertex<L: Lanes>(
    directional: &Directional,
    floor: [f32; 3],
    normal: [f32; 3],
    position: [f32; 3],
    lights: &[PointLight],
    ambient: [f32; 3],
) -> u32 {
    vertices::<L>(
        directional,
        floor,
        &[normal; 4],
        &[position; 4],
        lights,
        ambient,
    )[0]
}

/// [`vertex`] for four vertices at once, lane by lane: four independent vertices fill a vector register's lanes, and
/// each lane's result is [`vertex`]'s.
pub fn vertices<L: Lanes>(
    directional: &Directional,
    floor: [f32; 3],
    normals: &[[f32; 3]; 4],
    positions: &[[f32; 3]; 4],
    lights: &[PointLight],
    ambient: [f32; 3],
) -> [u32; 4] {
    let axis = |v: &[[f32; 3]; 4], k: usize| L::load([v[0][k], v[1][k], v[2][k], v[3][k]]);
    let (nx, ny, nz) = (axis(normals, 0), axis(normals, 1), axis(normals, 2));
    let (px, py, pz) = (axis(positions, 0), axis(positions, 1), axis(positions, 2));
    let [dx, dy, dz] = directional.direction.map(L::splat);
    let dot = (dx * nx + dy * ny) + dz * nz;
    let [cr, cg, cb] = directional.colour.map(L::splat);
    let mut red = larger(L::splat(floor[0]), (cr * dot).single());
    let mut green = larger(L::splat(floor[1]), (dot * cg).single());
    let mut blue = larger(L::splat(floor[2]), (dot * cb).single());
    for light in lights {
        let [lx, ly, lz] = light.position.map(L::splat);
        let (dx, dy, dz) = (lx - px, ly - py, lz - pz);
        let d2 = (dx * dx + dy * dy) + dz * dz;
        let fade = L::splat(1.0) - L::splat(light.falloff) * d2;
        // `fcom 0.0`, then a jump on C0: below 0, or NaN, the light is passed over.
        let reaches = fade.at_least(L::splat(0.0));
        if !L::any(reaches) {
            continue;
        }
        let inverse = d2.sqrt().recip();
        let share = (((dx * inverse) * nx + (dy * inverse) * ny) + (dz * inverse) * nz) * fade;
        let [lr, lg, lb] = light.colour.map(L::splat);
        // A lane the light does not reach keeps its value; the share computed for it is dropped.
        red = L::select(reaches, larger(red, (lr * share).single()), red);
        green = L::select(reaches, larger(green, (lg * share).single()), green);
        blue = L::select(reaches, larger(blue, (share * lb).single()), blue);
    }
    let channel = |value: L, ambient: f32| capped((value + L::splat(ambient)).single());
    let [alpha, ..] = bytes(L::splat(directional.alpha));
    let (r, g, b) = (
        bytes(channel(red, ambient[0])),
        bytes(channel(green, ambient[1])),
        bytes(channel(blue, ambient[2])),
    );
    from_fn(|i| alpha << 24 | r[i] << 16 | g[i] << 8 | b[i])
}

#[cfg(test)]
mod tests {
    use super::*;

    type Single = Scalar<f32>;
    type Double = Scalar<f64>;

    const WHITE_DOWN: Directional = Directional {
        direction: [0.0, 0.0, 1.0],
        colour: [1.0, 1.0, 1.0],
        alpha: 1.0,
    };

    #[test]
    fn a_normal_facing_the_light_is_lit_fully_and_one_facing_away_takes_the_floor() {
        let up = vertex::<Single>(
            &WHITE_DOWN,
            [0.1; 3],
            [0.0, 0.0, 1.0],
            [0.0; 3],
            &[],
            [0.0; 3],
        );
        assert_eq!(up, 0xFFFF_FFFF);
        let down = vertex::<Single>(
            &WHITE_DOWN,
            [0.1; 3],
            [0.0, 0.0, -1.0],
            [0.0; 3],
            &[],
            [0.0; 3],
        );
        // 0.1f * 255 is 25.5 exactly in single precision, which rounds to the even 26.
        assert_eq!(down, 0xFF1A_1A1A);
    }

    #[test]
    fn a_point_light_out_of_range_adds_nothing_and_one_in_range_adds_its_share() {
        let dark = Directional {
            colour: [0.0; 3],
            ..WHITE_DOWN
        };
        let mut light = PointLight {
            position: [0.0, 0.0, 2.0],
            falloff: 0.5,
            colour: [1.0, 0.5, 0.0],
        };
        // 1 - 0.5 * 4 < 0: out of range.
        assert_eq!(
            vertex::<Double>(
                &dark,
                [0.0; 3],
                [0.0, 0.0, 1.0],
                [0.0; 3],
                &[light],
                [0.0; 3]
            ),
            0xFF00_0000
        );
        // 1 - 0.125 * 4 = 0.5: red 0.5, green 0.25.
        light.falloff = 0.125;
        assert_eq!(
            vertex::<Double>(
                &dark,
                [0.0; 3],
                [0.0, 0.0, 1.0],
                [0.0; 3],
                &[light],
                [0.0; 3]
            ),
            0xFF80_4000
        );
    }

    #[test]
    fn a_lane_out_of_range_keeps_its_value_beside_one_in_range() {
        let dark = Directional {
            colour: [0.0; 3],
            ..WHITE_DOWN
        };
        let light = PointLight {
            position: [0.0, 0.0, 2.0],
            falloff: 0.125,
            colour: [1.0, 0.5, 0.0],
        };
        // Vertex 0 at the origin (in range), vertex 1 far away (out of range).
        let positions = [[0.0; 3], [0.0, 0.0, 100.0], [0.0; 3], [0.0; 3]];
        let colours = vertices::<Double>(
            &dark,
            [0.0; 3],
            &[[0.0, 0.0, 1.0]; 4],
            &positions,
            &[light],
            [0.0; 3],
        );
        assert_eq!(colours[..2], [0xFF80_4000, 0xFF00_0000]);
    }

    #[test]
    fn nan_picks_the_new_value_and_the_cap_keeps_nan() {
        let nan = Single::splat(f32::NAN);
        assert_eq!(larger(nan, Single::splat(0.5)).0[0], 0.5);
        assert!(larger(Single::splat(0.5), nan).0[0].is_nan());
        assert!(capped(nan).0[0].is_nan());
        assert_eq!(capped(Single::splat(1.5)).0[0], 1.0);
    }

    #[test]
    fn the_two_precisions_differ_where_the_x87_would() {
        // x*x is 1 + 2^-11 + 2^-24: single precision rounds the tie down to even, double keeps it, and the second
        // product then rounds to different singles (1 + 3*2^-12 + 2^-23 against + 2^-22).
        let x = 1.0 + 1.0 / 4096.0;
        let wide = (Double::splat(x) * Double::splat(x) * Double::splat(x)).floats();
        let narrow = (Single::splat(x) * Single::splat(x) * Single::splat(x)).floats();
        assert_ne!(wide, narrow);
    }
}
