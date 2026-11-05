//! The arithmetic functions. Each step rounds to float, as the
//! original's x87 code does at the game's 24-bit precision; sin, cos and pow are the C runtime's double functions,
//! rounded to float.
#![expect(unsafe_code, reason = "drawing from the game's random generator")]

use eix_core::script;
use eix_engine::game::flow;

use super::Call;

pub(super) fn sin(call: &Call) -> bool {
    call.set_float(f64::from(call.float(0)).sin() as f32);
    true
}

pub(super) fn cos(call: &Call) -> bool {
    call.set_float(f64::from(call.float(0)).cos() as f32);
    true
}

pub(super) fn pow(call: &Call) -> bool {
    call.set_float(f64::from(call.float(0)).powf(f64::from(call.float(1))) as f32);
    true
}

/// Four floats, though the signature has two: the original read the last two past the argument array (heap bytes);
/// here they are 0.
pub(super) fn distance_point_point(call: &Call) -> bool {
    call.set_float(script::distance(
        call.float(0),
        call.float(1),
        call.float(2),
        call.float(3),
    ));
    true
}

pub(super) fn add(call: &Call) -> bool {
    call.set_float(script::sum(call.floats()));
    true
}

pub(super) fn mul(call: &Call) -> bool {
    call.set_float(script::product(call.floats()));
    true
}

pub(super) fn and(call: &Call) -> bool {
    call.set_float(script::and(call.floats()));
    true
}

pub(super) fn or(call: &Call) -> bool {
    call.set_float(script::or(call.floats()));
    true
}

pub(super) fn max(call: &Call) -> bool {
    call.set_float(script::max(call.floats()));
    true
}

pub(super) fn min(call: &Call) -> bool {
    call.set_float(script::min(call.floats()));
    true
}

pub(super) fn abs(call: &Call) -> bool {
    call.set_float(script::abs(call.float(0)));
    true
}

pub(super) fn round(call: &Call) -> bool {
    call.set_float(script::round(call.float(0)));
    true
}

pub(super) fn ceil(call: &Call) -> bool {
    call.set_float(f64::from(call.float(0)).ceil() as f32);
    true
}

pub(super) fn floor(call: &Call) -> bool {
    call.set_float(f64::from(call.float(0)).floor() as f32);
    true
}

/// `Random(max)` is the game's own (declined); `Random(min, max)` is the generator's range when max > min, else min.
pub(super) fn random(call: &Call) -> bool {
    if call.floats().len() == 1 {
        return false;
    }
    let (low, high) = (call.float(0), call.float(1));
    let value = if high > low {
        // SAFETY: script functions run on the game's thread, with the generator constructed.
        unsafe { flow::random_range(low, high) }
    } else {
        low
    };
    call.set_float(value);
    true
}
