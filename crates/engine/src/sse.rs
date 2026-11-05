//! SSE2 vectors of four floats: four `f32` in one register, four `f64` in two. Each lane's arithmetic is the scalar
//! IEEE operation (round to nearest under the default MXCSR), so four values computed side by side equal four computed
//! one at a time.
#![expect(
    unsafe_code,
    reason = "SSE2 intrinsics: part of the i686 target's baseline, but `unsafe` to call outside a target-feature function"
)]

use core::ops::{Add, Div, Mul, Sub};

use core::arch::x86::{
    __m128, __m128d, _mm_add_pd, _mm_add_ps, _mm_and_pd, _mm_and_ps, _mm_andnot_pd, _mm_andnot_ps,
    _mm_cmpge_pd, _mm_cmpge_ps, _mm_cvtpd_ps, _mm_cvtps_pd, _mm_div_pd, _mm_div_ps, _mm_movehl_ps,
    _mm_movelh_ps, _mm_movemask_pd, _mm_movemask_ps, _mm_mul_pd, _mm_mul_ps, _mm_or_pd, _mm_or_ps,
    _mm_set_ps, _mm_set1_pd, _mm_set1_ps, _mm_sqrt_pd, _mm_sqrt_ps, _mm_storeu_ps, _mm_sub_pd,
    _mm_sub_ps,
};

const _: () = assert!(cfg!(target_feature = "sse2"), "the target must have SSE2");

/// Four `f32`.
#[derive(Debug, Clone, Copy)]
pub struct F32x4(__m128);

/// A lane mask of an [`F32x4`] comparison.
#[derive(Debug, Clone, Copy)]
pub struct Mask32(__m128);

/// Four `f64`, in two registers.
#[derive(Debug, Clone, Copy)]
pub struct F64x4(__m128d, __m128d);

/// A lane mask of an [`F64x4`] comparison.
#[derive(Debug, Clone, Copy)]
pub struct Mask64(__m128d, __m128d);

/// Lane-wise `+ - * /` for both vector types.
macro_rules! operators {
    ($($trait:ident $method:ident $ps:ident $pd:ident;)*) => {
        $(
            impl $trait for F32x4 {
                type Output = Self;
                fn $method(self, o: Self) -> Self {
                    // SAFETY: SSE2 is enabled (asserted above); no memory is touched.
                    Self(unsafe { $ps(self.0, o.0) })
                }
            }

            impl $trait for F64x4 {
                type Output = Self;
                fn $method(self, o: Self) -> Self {
                    // SAFETY: as above.
                    let lo = unsafe { $pd(self.0, o.0) };
                    // SAFETY: as above.
                    Self(lo, unsafe { $pd(self.1, o.1) })
                }
            }
        )*
    };
}

operators! {
    Add add _mm_add_ps _mm_add_pd;
    Sub sub _mm_sub_ps _mm_sub_pd;
    Mul mul _mm_mul_ps _mm_mul_pd;
    Div div _mm_div_ps _mm_div_pd;
}

impl F32x4 {
    /// `x` in every lane.
    pub fn splat(x: f32) -> Self {
        // SAFETY: SSE2 is enabled (asserted above); no memory is touched.
        Self(unsafe { _mm_set1_ps(x) })
    }

    /// The four values, lane 0 first.
    pub fn from_array(x: [f32; 4]) -> Self {
        // SAFETY: as in `splat`.
        Self(unsafe { _mm_set_ps(x[3], x[2], x[1], x[0]) })
    }

    /// The four lanes, lane 0 first.
    pub fn to_array(self) -> [f32; 4] {
        let mut out = [0.0; 4];
        // SAFETY: `out` has room for the four floats the store writes.
        unsafe { _mm_storeu_ps(out.as_mut_ptr(), self.0) };
        out
    }

    /// Lane-wise square root.
    #[must_use]
    pub fn sqrt(self) -> Self {
        // SAFETY: as in `splat`.
        Self(unsafe { _mm_sqrt_ps(self.0) })
    }

    /// Where `self >= o`; a NaN on either side is not.
    pub fn at_least(self, o: Self) -> Mask32 {
        // SAFETY: as in `splat`.
        Mask32(unsafe { _mm_cmpge_ps(self.0, o.0) })
    }

    /// `yes` in the lanes `mask` has, `no` in the others: each lane's bits as they are.
    pub fn select(mask: Mask32, yes: Self, no: Self) -> Self {
        // SAFETY: as in `splat`.
        let a = unsafe { _mm_and_ps(mask.0, yes.0) };
        // SAFETY: as in `splat`.
        let b = unsafe { _mm_andnot_ps(mask.0, no.0) };
        // SAFETY: as in `splat`.
        Self(unsafe { _mm_or_ps(a, b) })
    }

    /// Each lane as an `f64` (exact).
    pub fn widen(self) -> F64x4 {
        // SAFETY: as in `splat`.
        let high = unsafe { _mm_movehl_ps(self.0, self.0) };
        // SAFETY: as in `splat`.
        let lo = unsafe { _mm_cvtps_pd(self.0) };
        // SAFETY: as in `splat`.
        let hi = unsafe { _mm_cvtps_pd(high) };
        F64x4(lo, hi)
    }
}

impl Mask32 {
    /// Whether any lane is set.
    pub fn any(self) -> bool {
        // SAFETY: as in `F32x4::splat`.
        (unsafe { _mm_movemask_ps(self.0) }) != 0
    }
}

impl F64x4 {
    /// `x` in every lane.
    pub fn splat(x: f64) -> Self {
        // SAFETY: as in `F32x4::splat`.
        let v = unsafe { _mm_set1_pd(x) };
        Self(v, v)
    }

    /// Each lane rounded to the nearest `f32`.
    pub fn narrow(self) -> F32x4 {
        // SAFETY: as in `F32x4::splat`.
        let lo = unsafe { _mm_cvtpd_ps(self.0) };
        // SAFETY: as in `F32x4::splat`.
        let hi = unsafe { _mm_cvtpd_ps(self.1) };
        // Each conversion fills the low two lanes; the high two of `hi` go above `lo`'s.
        // SAFETY: as in `F32x4::splat`.
        F32x4(unsafe { _mm_movelh_ps(lo, hi) })
    }

    /// Lane-wise square root.
    #[must_use]
    pub fn sqrt(self) -> Self {
        // SAFETY: as in `F32x4::splat`.
        let lo = unsafe { _mm_sqrt_pd(self.0) };
        // SAFETY: as in `F32x4::splat`.
        Self(lo, unsafe { _mm_sqrt_pd(self.1) })
    }

    /// Where `self >= o`; a NaN on either side is not.
    pub fn at_least(self, o: Self) -> Mask64 {
        // SAFETY: as in `F32x4::splat`.
        let lo = unsafe { _mm_cmpge_pd(self.0, o.0) };
        // SAFETY: as in `F32x4::splat`.
        Mask64(lo, unsafe { _mm_cmpge_pd(self.1, o.1) })
    }

    /// `yes` in the lanes `mask` has, `no` in the others: each lane's bits as they are.
    pub fn select(mask: Mask64, yes: Self, no: Self) -> Self {
        let pick = |m: __m128d, y: __m128d, n: __m128d| {
            // SAFETY: as in `F32x4::splat`.
            let a = unsafe { _mm_and_pd(m, y) };
            // SAFETY: as in `F32x4::splat`.
            let b = unsafe { _mm_andnot_pd(m, n) };
            // SAFETY: as in `F32x4::splat`.
            unsafe { _mm_or_pd(a, b) }
        };
        Self(pick(mask.0, yes.0, no.0), pick(mask.1, yes.1, no.1))
    }
}

impl Mask64 {
    /// Whether any lane is set.
    pub fn any(self) -> bool {
        // SAFETY: as in `F32x4::splat`.
        let lo = unsafe { _mm_movemask_pd(self.0) };
        // SAFETY: as in `F32x4::splat`.
        let hi = unsafe { _mm_movemask_pd(self.1) };
        (lo | hi) != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lanes_are_the_scalar_operations() {
        let a = [1.5f32, -2.0, 1e-3, 7.0];
        let b = [0.25f32, 3.0, 9.0, -7.0];
        let (va, vb) = (F32x4::from_array(a), F32x4::from_array(b));
        assert_eq!((va + vb).to_array(), core::array::from_fn(|i| a[i] + b[i]));
        assert_eq!((va / vb).to_array(), core::array::from_fn(|i| a[i] / b[i]));
        assert_eq!(
            vb.sqrt().to_array().map(f32::to_bits),
            b.map(|x| x.sqrt().to_bits())
        );
        let wide = (va.widen() * vb.widen()).narrow().to_array();
        assert_eq!(
            wide,
            core::array::from_fn(|i| (f64::from(a[i]) * f64::from(b[i])) as f32)
        );
    }

    #[test]
    fn a_nan_is_never_at_least_and_select_keeps_bits() {
        let v = F32x4::from_array([f32::NAN, 1.0, 2.0, -0.0]);
        let m = v.at_least(F32x4::splat(1.0));
        assert!(m.any());
        let picked = F32x4::select(m, F32x4::splat(9.0), v).to_array();
        assert!(picked[0].is_nan());
        assert_eq!(&picked[1..], &[9.0, 9.0, -0.0]);
        assert_eq!(picked[3].to_bits(), (-0.0f32).to_bits());
        assert!(!F32x4::splat(0.0).at_least(F32x4::splat(1.0)).any());
    }
}
