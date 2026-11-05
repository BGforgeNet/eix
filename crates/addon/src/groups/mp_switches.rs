//! The multiplayer switches of the starter's settings, each a byte patch of game.exe's own tests:
//!
//! - MQClient (`MQ client`): `ClientOnFileChunk` 0x41ECC0 returns once every chunk is collected, so no file the server
//!   sends is written (the `jne` at 0x41ED35 falls through, the `je` at 0x41ED40 always jumps).
//! - MPSpeedControl (`MP Speed-control`): the network test of the two game-speed keys in `BattleScreenKeyDown`
//!   0x63A560 and of the clock's speed buttons in 0x648D70 jumps to the next instruction (`jne +0`).
//! - MPConsole (`MP console`): the console's two tests for a network game in `Execute` 0x5DC620 become `mov eax, 0`,
//!   so its commands work in one.

use super::{BuildError, Labelled, Setup, bytes};

const JNE_NEXT: [u8; 6] = [0x0F, 0x85, 0, 0, 0, 0];
const MOV_EAX_0: [u8; 5] = [0xB8, 0, 0, 0, 0];

pub(super) fn mq_client(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    Ok(vec![
        bytes(
            "MQClient #1",
            eix_sites::CLIENT_ON_FILE_CHUNK__76,
            &[0x0F],
            &[0x00],
        )?,
        bytes(
            "MQClient #2",
            eix_sites::CLIENT_ON_FILE_CHUNK__80,
            &[0x0F, 0x84, 0xB5, 0x01, 0x00, 0x00],
            &[0xE9, 0xB6, 0x01, 0x00, 0x00, 0x90],
        )?,
    ])
}

pub(super) fn mp_speed_control(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    Ok(vec![
        bytes(
            "MPSpeedControl #1 keyboard",
            eix_sites::BATTLE_SCREEN_KEY_DOWN__174,
            &[0x0F, 0x85, 0xEA, 0x0D, 0x00, 0x00],
            &JNE_NEXT,
        )?,
        bytes(
            "MPSpeedControl #2 keyboard",
            eix_sites::BATTLE_SCREEN_KEY_DOWN__1DA,
            &[0x0F, 0x85, 0x84, 0x0D, 0x00, 0x00],
            &JNE_NEXT,
        )?,
        bytes(
            "MPSpeedControl #3 mouse",
            eix_sites::CLOCK_MOUSE_UP_VERIFY__8E,
            &[0x0F, 0x85, 0xBF, 0x00, 0x00, 0x00],
            &JNE_NEXT,
        )?,
        bytes(
            "MPSpeedControl #4 mouse",
            eix_sites::CLOCK_MOUSE_UP_VERIFY__F5,
            &[0x75, 0x5C],
            &[0x75, 0x00],
        )?,
    ])
}

pub(super) fn mp_console(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    Ok(vec![
        bytes(
            "MPConsole #1",
            eix_sites::TEXT_5DD558,
            &[0xE8, 0xE3, 0x1E, 0xFA, 0xFF],
            &MOV_EAX_0,
        )?,
        bytes(
            "MPConsole #2",
            eix_sites::TEXT_5DD780,
            &[0xE8, 0xBB, 0x1C, 0xFA, 0xFF],
            &MOV_EAX_0,
        )?,
    ])
}
