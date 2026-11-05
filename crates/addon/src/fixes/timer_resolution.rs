//! TimerResolution: the process asks for a 1 ms system timer. game.exe times its frames and ticks with `timeGetTime`
//! alone and never calls `timeBeginPeriod`, so where a process gets the default resolution (Windows 10 2004 and
//! later), its clock moves in steps of about 15.6 ms.

use eix_engine::win::raise_timer_resolution;

use crate::groups::{BuildError, Labelled};
use crate::setup::Setup;

/// The period asked for, in milliseconds.
const PERIOD_MS: u32 = 1;

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    if !raise_timer_resolution(PERIOD_MS) {
        return Err(format!("timeBeginPeriod({PERIOD_MS}) was refused").into());
    }
    Ok(Vec::new())
}
