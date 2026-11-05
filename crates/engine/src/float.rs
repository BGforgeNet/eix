//! Float to int as the original addon and the game's MSVC runtime convert: the one helper for game-state code.

/// A number truncated to an int as MSVC's `_ftol2_sse` (`cvttsd2si`) and `_ftol` do: toward zero, and NaN or a value
/// outside the int range gives 0x80000000 (the x87 "integer indefinite").
///
/// addon.dll 0.12.4.2 converts this way in `luaL_checkinteger` / `lua_tointeger` (so `UInt32ToMem(a, 4294967295)`
/// writes 0x80000000) and wherever it stores a float into an int field (`_ftol2_sse`).
///
/// `eix_core::num::trunc_to_i32` is a copy for platform-free code; a test pins the two together.
pub fn ftol(x: f64) -> i32 {
    // Both bounds are exact doubles; NaN fails both comparisons.
    if x > -2_147_483_649.0 && x < 2_147_483_648.0 {
        x as i32
    } else {
        i32::MIN
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncates_and_saturates_to_the_indefinite_integer() {
        assert_eq!(ftol(2.9), 2);
        assert_eq!(ftol(-2.9), -2);
        assert_eq!(ftol(2_147_483_647.9), i32::MAX);
        assert_eq!(ftol(-2_147_483_648.9), i32::MIN);
        assert_eq!(ftol(2_147_483_648.0), i32::MIN);
        assert_eq!(ftol(4_294_967_295.0), i32::MIN);
        assert_eq!(ftol(f64::NAN), i32::MIN);
        assert_eq!(ftol(f64::NEG_INFINITY), i32::MIN);
    }

    /// `eix_core::num::trunc_to_i32` is this function's copy for platform-free code; they must agree.
    #[test]
    fn agrees_with_cores_copy() {
        let edges = [
            0.0,
            -0.9,
            12.9,
            -12.9,
            2_147_483_647.9,
            2_147_483_648.0,
            -2_147_483_648.9,
            -2_147_483_649.0,
            4_294_967_295.0,
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ];
        for x in edges {
            assert_eq!(ftol(x), eix_core::num::trunc_to_i32(x), "{x}");
        }
    }
}
