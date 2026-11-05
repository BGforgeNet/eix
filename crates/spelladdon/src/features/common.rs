//! What the features' hook handlers share. The original's added code runs in game.exe's context, so a handler that
//! does its work reads and writes game memory and the x87 stack the replaced instructions leave: fixed byte tables,
//! parameters written over game constants or padding, sites whose patched bytes keep the original's layout with the
//! branch aimed at a stub of ours, stack words read below the stack pointer, the x87 top, the game memory itself, and
//! player 0's script variables. Flags come from [`eix_core::x86_flags`], x87 arithmetic from
//! [`eix_engine::x87`].
#![expect(
    unsafe_code,
    reason = "hook handlers read and write game memory and the x87 stack as the original's added code does"
)]

use core::ffi::CStr;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use eix_engine::exec;
use eix_engine::game::mfc::{self, GameString};
use eix_engine::game::player::{NamedFloatTable, Player, PlayerServer};
use eix_engine::game::world::{SERVER_WORLD, World};
use eix_engine::group::SiteWrite;
use eix_engine::hook::{self, Context, Handler, HookError, Mode};
use eix_engine::patch;

use super::{BuildError, Ctx};

/// One fixed write: the site, game.exe's bytes there, and what replaces them.
// Tables of these stay `const` beside their feature rather than in a data file: the compiler checks them, and the
// feature's handlers, notes and `assert_original` runs sit in the same module.
pub(crate) type Bytes = (usize, &'static [u8], &'static [u8]);

/// The writes of a table of fixed bytes.
pub(crate) fn fixed(table: &[Bytes]) -> Result<Vec<SiteWrite>, BuildError> {
    table
        .iter()
        .map(|&(site, was, now)| Ok(SiteWrite::bytes(site, was, now)?))
        .collect()
}

/// `[gameplay] key` as a 32-bit float over the game's constant `was` at `site`.
pub(crate) fn float(
    ctx: &Ctx<'_>,
    key: &str,
    site: usize,
    was: f32,
) -> Result<SiteWrite, BuildError> {
    float_over(ctx, key, site, was.to_le_bytes())
}

/// `[gameplay] key` as a 32-bit float over the bytes `was` (padding, or a constant) at `site`.
pub(crate) fn float_over(
    ctx: &Ctx<'_>,
    key: &str,
    site: usize,
    was: [u8; 4],
) -> Result<SiteWrite, BuildError> {
    let now = ctx.values.float("gameplay", key)?;
    Ok(SiteWrite::bytes(site, &was, &now.to_le_bytes())?)
}

/// NOPs over `was` at each site.
pub(crate) fn nops(table: &[(usize, &[u8])]) -> Vec<SiteWrite> {
    table
        .iter()
        .map(|&(site, was)| SiteWrite::nop(site, was))
        .collect()
}

/// A 32-bit float the game reads at `site`, which held `was`.
pub(crate) fn float_at(site: usize, was: f32, now: f32) -> Result<SiteWrite, BuildError> {
    Ok(SiteWrite::bytes(
        site,
        &was.to_le_bytes(),
        &now.to_le_bytes(),
    )?)
}

/// The site's patched bytes `now`, the original's layout, whose `jmp` or `call` at offset `at` (E9 or E8, then a
/// placeholder displacement) goes to `to`.
pub(crate) fn branch(
    site: usize,
    was: &[u8],
    now: &[u8],
    at: usize,
    to: usize,
) -> Result<SiteWrite, BuildError> {
    let mut new = now.to_vec();
    let rel = match new.get(at) {
        Some(0xE9) => patch::jmp_rel32(site + at, to),
        Some(0xE8) => patch::call_rel32(site + at, to),
        other => {
            return Err(BuildError(format!(
                "site {site:#x}: no jmp or call at +{at} of the new bytes ({other:02X?})"
            )));
        }
    };
    new.get_mut(at..at + 5)
        .ok_or_else(|| BuildError(format!("site {site:#x}: the branch at +{at} overruns")))?
        .copy_from_slice(&rel);
    Ok(SiteWrite::bytes(site, was, &new)?)
}

/// A stub entered by a jump: `before` (code taken from `origin`), `handler(arg, ..)`, then `resume` unless the
/// handler changes it.
pub(crate) fn jump_stub(
    origin: usize,
    before: &[u8],
    handler: Handler,
    arg: usize,
    resume: usize,
) -> Result<usize, BuildError> {
    Ok(hook::stub(
        origin,
        before,
        &[],
        false,
        handler,
        arg,
        resume,
    )?)
}

/// The site's patched bytes `now` in the original's layout, whose `jmp` or `call` at `at` goes to a stub that runs
/// the replaced instructions `was` first, then `handler(arg, ..)`, and goes on after the site unless the handler
/// changes `resume`. For a handler that does the replaced instructions' work and then its own.
pub(crate) fn branch_before(
    site: usize,
    was: &[u8],
    now: &[u8],
    at: usize,
    handler: Handler,
    arg: usize,
) -> Result<SiteWrite, BuildError> {
    let by_call = now.get(at) == Some(&0xE8);
    let stub = hook::stub(site, was, &[], by_call, handler, arg, site + was.len())?;
    branch(site, was, now, at, stub)
}

/// A stub entered by a call, which goes on at `resume` (the instruction after the call) unless the handler changes
/// it: the handler's `ret`.
pub(crate) fn call_stub(handler: Handler, arg: usize, resume: usize) -> Result<usize, BuildError> {
    Ok(hook::stub(0, &[], &[], true, handler, arg, resume)?)
}

/// A stub entered by a jump that runs `handler(arg, ..)`, then `after` (code taken from `origin`), which must leave
/// by itself (a `ret`, a jump).
pub(crate) fn tail_stub(
    origin: usize,
    handler: Handler,
    arg: usize,
    after: &[u8],
) -> Result<usize, BuildError> {
    Ok(hook::stub(origin, &[], after, false, handler, arg, 0)?)
}

/// Stack words a handler reads below the stack pointer, copied before our stub's own pushes overwrite them.
pub(crate) struct Stale<const N: usize> {
    words: [AtomicU32; N],
    /// EAX while the copy runs.
    eax: AtomicU32,
}

impl<const N: usize> Stale<N> {
    pub(crate) const fn new() -> Self {
        Self {
            words: [const { AtomicU32::new(0) }; N],
            eax: AtomicU32::new(0),
        }
    }

    /// Code that copies the words at ESP + `offsets[i]` (the stack at the site) into word `i`, and changes no
    /// register or flag.
    pub(crate) fn copy(&'static self, offsets: [i32; N]) -> Vec<u8> {
        let eax = self.eax.as_ptr() as u32;
        let mut code = vec![0xA3];
        code.extend_from_slice(&eax.to_le_bytes());
        for (word, offset) in self.words.iter().zip(offsets) {
            // mov eax, [esp + offset]; mov [word], eax
            code.extend_from_slice(&[0x8B, 0x84, 0x24]);
            code.extend_from_slice(&offset.to_le_bytes());
            code.push(0xA3);
            code.extend_from_slice(&(word.as_ptr() as u32).to_le_bytes());
        }
        code.push(0xA1);
        code.extend_from_slice(&eax.to_le_bytes());
        code
    }

    /// Word `i` as last copied.
    pub(crate) fn word(&self, i: usize) -> u32 {
        self.words[i].load(Ordering::Relaxed)
    }

    /// Word `i` as a 32-bit float.
    pub(crate) fn float(&self, i: usize) -> f32 {
        f32::from_bits(self.word(i))
    }
}

/// An x87 register's value moved through memory around a handler: the stub's code stores ST0 here and pops it
/// (`fstp qword`), the handler reads and replaces the value, the code after it loads it back (`fld qword`). An `f64`
/// holds the register exactly unless the precision control is 64 bits.
pub(crate) struct FpuSlot(AtomicU64);

impl FpuSlot {
    pub(crate) const fn new() -> Self {
        Self(AtomicU64::new(0))
    }

    fn operand(&'static self, opcode: [u8; 2]) -> Vec<u8> {
        let mut code = opcode.to_vec();
        code.extend_from_slice(&(self.0.as_ptr() as u32).to_le_bytes());
        code
    }

    /// `fstp qword [slot]`.
    pub(crate) fn store(&'static self) -> Vec<u8> {
        self.operand([0xDD, 0x1D])
    }

    /// `fst qword [slot]`: a copy, ST0 stays.
    pub(crate) fn copy(&'static self) -> Vec<u8> {
        self.operand([0xDD, 0x15])
    }

    /// `fld qword [slot]`.
    pub(crate) fn load(&'static self) -> Vec<u8> {
        self.operand([0xDD, 0x05])
    }

    pub(crate) fn get(&self) -> f64 {
        f64::from_bits(self.0.load(Ordering::Relaxed))
    }

    pub(crate) fn set(&self, value: f64) {
        self.0.store(value.to_bits(), Ordering::Relaxed);
    }
}

/// A game object a handler reads and writes by offset: the bytes `at .. at + len`, which its creator vouched for.
///
/// An offset outside the object is a defect in the handler; it aborts the game rather than touch other memory.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Obj {
    at: u32,
    len: u32,
}

impl Obj {
    /// The object at `at`, `len` bytes long.
    ///
    /// # Safety
    /// `at .. at + len` must be mapped game memory that no other thread uses while the `Obj` is in use, and the
    /// handler's reads and writes must be ones the game's own code makes there.
    pub(crate) unsafe fn new(at: u32, len: u32) -> Self {
        Self { at, len }
    }

    /// Its address.
    pub(crate) fn addr(self) -> u32 {
        self.at
    }

    fn place<T>(self, offset: u32) -> u32 {
        let size = size_of::<T>() as u32;
        assert!(
            offset.checked_add(size).is_some_and(|end| end <= self.len),
            "offset {offset:#x} outside an object of {:#x} bytes",
            self.len
        );
        self.at.wrapping_add(offset)
    }

    /// The `T` at `offset`.
    pub(crate) fn get<T: Copy>(self, offset: u32) -> T {
        let at = self.place::<T>(offset);
        // SAFETY: `place` kept the read inside the object `new`'s caller vouched for.
        unsafe { read(at) }
    }

    /// Stores `value` at `offset`.
    pub(crate) fn set<T: Copy>(self, offset: u32, value: T) {
        let at = self.place::<T>(offset);
        // SAFETY: as in `get`, for a write the game's own code makes there.
        unsafe { write(at, value) }
    }
}

/// The x87 stack top, `st(0)`, rounded to a float under the control word; the stack is left as it was.
pub(crate) fn st0() -> f32 {
    let mut out = 0f32;
    // SAFETY: `fst` stores st(0) without popping, so the x87 stack is unchanged; the hooked sites hold a value there.
    unsafe {
        core::arch::asm!(
            "fst dword ptr [{p}]",
            p = in(reg) &raw mut out,
            options(nostack, preserves_flags)
        );
    }
    out
}

/// Replaces `st(0)` with `value`, keeping the stack's depth.
pub(crate) fn set_st0(value: f32) {
    // SAFETY: `fstp st(0)` drops the top and `fld` pushes the new value, so the depth is the same and only the value
    // the replaced instructions leave there changes.
    unsafe {
        core::arch::asm!(
            "fstp st(0)",
            "fld dword ptr [{p}]",
            p = in(reg) &raw const value,
            options(nostack, preserves_flags)
        );
    }
}

/// A context hook entered by a jump at `site + at`, with the bytes before it NOPed: the layout of the original's
/// `nop ...; jmp <hook>`. `expected` is the whole site; the handler does the work of every instruction in it.
pub(crate) fn jump_at(
    site: usize,
    expected: &[u8],
    at: usize,
    handler: Handler,
    arg: usize,
) -> Result<Vec<SiteWrite>, HookError> {
    let mut writes = Vec::with_capacity(2);
    if at > 0 {
        writes.push(SiteWrite::nop(site, &expected[..at]));
    }
    writes.push(hook::context_hook(
        site + at,
        &expected[at..],
        Mode::Drop,
        handler,
        arg,
    )?);
    Ok(writes)
}

/// [`jump_at`] entered by a `call`, for a site the original turns into `nop ...; call <hook>`.
pub(crate) fn call_at(
    site: usize,
    expected: &[u8],
    at: usize,
    handler: Handler,
    arg: usize,
) -> Result<Vec<SiteWrite>, HookError> {
    let mut writes = Vec::with_capacity(2);
    if at > 0 {
        writes.push(SiteWrite::nop(site, &expected[..at]));
    }
    writes.push(hook::context_call(
        site + at,
        &expected[at..],
        Mode::Drop,
        handler,
        arg,
    )?);
    Ok(writes)
}

/// The address of a context stub that runs `handler(arg, context)` and goes on at the handler's `resume`, for a
/// branch the original points at added code (a `jcc` displacement, a table entry). The handler must set `resume`.
pub(crate) fn entry(handler: Handler, arg: usize) -> Result<usize, HookError> {
    hook::stub(0, &[], &[], false, handler, arg, 0)
}

/// The `rel32` of a branch whose displacement ends at `end` and leads to `to`.
pub(crate) fn rel32(end: usize, to: usize) -> [u8; 4] {
    (to as u32).wrapping_sub(end as u32).to_le_bytes()
}

/// Player 0, whose variables the scripts reach with `GSGetVar(0, ...)` and `GSSetVar(0, ...)`, when the server world
/// has it.
pub(crate) fn player_zero() -> Option<*mut Player> {
    // SAFETY: the game's server world, constructed by its static initialiser; handlers run on the game's thread.
    let player = unsafe { World::get_player(SERVER_WORLD.as_ptr(), 0) };
    (!player.is_null()).then_some(player)
}

/// Switches the zone music to the local player's map: that map's record (`LocalPlayerName`, `CMapDatabase::FindMap`)
/// gives its word at `field` (a theme's name) to `CMusicSystem::SetZoneMode` with `mode`, then the music system's +0x44
/// becomes `flag` and +0x38 `level`. Nothing without the music system or the map; returns whether it switched.
pub(crate) fn zone_music(field: u32, mode: i32, flag: u32, level: u32) -> bool {
    type LocalPlayerName =
        unsafe extern "thiscall" fn(this: u32, out: *mut GameString) -> *mut GameString;
    type FindMap = unsafe extern "thiscall" fn(this: u32, name: *const u8) -> u32;
    type SetZoneMode = unsafe extern "thiscall" fn(this: u32, name: u32, mode: i32);
    // SAFETY: the music system's global, `[0x7AF760]` (the client world's +0x1A8).
    let music = unsafe { read::<u32>(eix_sites::DATA_7AF760 as u32) };
    if music == 0 {
        return false;
    }
    let mut name = core::mem::MaybeUninit::<GameString>::uninit();
    // SAFETY: 0x578A90 is `LocalPlayerName` (thiscall on 0x7AFEA5); it constructs the string at `name`.
    let local_player_name = unsafe { hook::as_fn::<LocalPlayerName>(eix_sites::LOCAL_PLAYER_NAME) };
    // SAFETY: as above, on the game's thread.
    let built = unsafe { local_player_name(eix_sites::DATA_7AFEA5 as u32, name.as_mut_ptr()) };
    // SAFETY: the constructed string's first word is its characters.
    let chars = unsafe { read::<u32>(built.expose_provenance() as u32) };
    // SAFETY: 0x45E850 is `CMapDatabase::FindMap` (thiscall, the map's name; its record or null).
    let find_map = unsafe { hook::as_fn::<FindMap>(eix_sites::C_MAP_DATABASE_FIND_MAP) };
    // SAFETY: the map database is a static object; `chars` is NUL-terminated.
    let record = unsafe {
        find_map(
            eix_sites::DATA_79BF00 as u32,
            core::ptr::with_exposed_provenance(chars as usize),
        )
    };
    // SAFETY: `name` was constructed above and is not used again.
    unsafe { mfc::destroy(name.as_mut_ptr()) };
    if record == 0 {
        return false;
    }
    // SAFETY: a map record's first words are its names' characters.
    let theme = unsafe { read::<u32>(record + field) };
    // SAFETY: 0x6B4E30 is `CMusicSystem::SetZoneMode` (thiscall, the theme's name and the mode; `ret 8`).
    let set_zone_mode =
        unsafe { hook::as_fn::<SetZoneMode>(eix_sites::C_MUSIC_SYSTEM_SET_ZONE_MODE) };
    // SAFETY: the live music system, on the game's thread.
    unsafe { set_zone_mode(music, theme, mode) };
    // SAFETY: fields of the live music system: +0x44 the zone-mode flag, +0x38 its level.
    unsafe { write::<u32>(music + 0x44, flag) };
    // SAFETY: as above.
    unsafe { write::<u32>(music + 0x38, level) };
    true
}

/// Whether `id` is a hero's: `IsTextKey` 0x4613A0, which takes the id in ECX and only compares it.
pub(crate) fn is_hero_id(id: u32) -> bool {
    type IsTextKey = unsafe extern "fastcall" fn(id: u32) -> u32;
    // SAFETY: 0x4613A0 is `BOOL __fastcall IsTextKey(id)`.
    let is_text_key = unsafe { hook::as_fn::<IsTextKey>(eix_sites::IS_TEXT_KEY) };
    // SAFETY: it reads nothing but the id.
    unsafe { is_text_key(id) != 0 }
}

/// Whether `unit` has a player: `CUnit::HasPlayer` 0x5301A0.
///
/// # Safety
/// `unit` must be a live `CUnit`, on the game's thread.
pub(crate) unsafe fn has_player(unit: u32) -> bool {
    type HasPlayer = unsafe extern "thiscall" fn(unit: u32) -> u32;
    // SAFETY: 0x5301A0 is `BOOL __thiscall CUnit::HasPlayer()`.
    let has_player = unsafe { hook::as_fn::<HasPlayer>(eix_sites::C_UNIT_HAS_PLAYER) };
    // SAFETY: the caller vouches for the unit.
    unsafe { has_player(unit) != 0 }
}

/// The longest variable name [`id_variable`] makes: a prefix of up to four bytes, an `i32` with its sign, the NUL.
pub(crate) const ID_NAME_LEN: usize = 4 + 11 + 1;

/// The variable named `prefix` and the id `id` as the original's `itoa` writes it (base 10, a leading `-` for a
/// negative id). `prefix` has at most four bytes.
pub(crate) fn id_variable<'a>(prefix: &[u8], id: i32, out: &'a mut [u8; ID_NAME_LEN]) -> &'a CStr {
    out.fill(0);
    let mut at = prefix.len().min(4);
    out[..at].copy_from_slice(&prefix[..at]);
    let mut digits = [0u8; 10];
    let mut n = id.unsigned_abs();
    let mut first = digits.len();
    loop {
        first -= 1;
        digits[first] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    if id < 0 {
        out[at] = b'-';
        at += 1;
    }
    let rest = digits.len() - first;
    out[at..at + rest].copy_from_slice(&digits[first..]);
    CStr::from_bytes_until_nul(&out[..]).unwrap_or(c"")
}

/// Player 0's variable `name` by the game's own lookup (`NamedFloatTable::Get`, case-insensitive, as `GSGetVar`
/// reads it), 0 when it or the player is missing.
pub(crate) fn get_variable(name: &CStr) -> f32 {
    let Some(player) = player_zero() else {
        return 0.0;
    };
    // SAFETY: `player` is a live `CPlayer`, whose +0xC4 is its variable table.
    let variables = unsafe { &raw mut (*player).variables };
    // SAFETY: that table, on the game's thread; `name` is NUL-terminated.
    unsafe { NamedFloatTable::lookup(variables, name) }
}

/// Sets player 0's variable `name` as `GSSetVar` does (`CPlayerServer::SetVariable`, which also sends it); nothing
/// without the player or its server half.
pub(crate) fn set_variable(name: &CStr, value: f32) {
    let Some(player) = player_zero() else {
        return;
    };
    // SAFETY: a live `CPlayer`'s +0x20, its server half or null.
    let server = unsafe { (*player).server };
    if !server.is_null() {
        // SAFETY: a live player's server half, on the game's thread; `name` is NUL-terminated.
        unsafe { PlayerServer::set_variable(server, name, value) };
    }
}

/// Keeps `value` for the life of the process and returns its address, a handler's `arg`.
pub(crate) fn keep<T: Send + Sync + 'static>(value: T) -> usize {
    Box::leak(Box::new(value)) as *const T as usize
}

/// The value [`keep`] stored at `arg`.
///
/// # Safety
/// `arg` must come from `keep::<T>`.
pub(crate) unsafe fn kept<T>(arg: usize) -> &'static T {
    // SAFETY: `keep` leaked a `T` at this address, which is never freed.
    unsafe { &*(arg as *const T) }
}

/// A value of game memory at `address`, as the original's code reads it: unaligned, of the type the instruction reads.
///
/// # Safety
/// `address` must be readable game memory holding a `T`, on the game's thread.
pub(crate) unsafe fn read<T: Copy>(address: u32) -> T {
    // SAFETY: the caller guarantees the address.
    unsafe { core::ptr::with_exposed_provenance::<T>(address as usize).read_unaligned() }
}

/// Writes `value` into game memory at `address`.
///
/// # Safety
/// `address` must be writable game memory holding a `T` that the game lets the hook change, on the game's thread.
pub(crate) unsafe fn write<T: Copy>(address: u32, value: T) {
    // SAFETY: the caller guarantees the address.
    unsafe { core::ptr::with_exposed_provenance_mut::<T>(address as usize).write_unaligned(value) }
}

/// Code that pushes the words a handler leaves in its slots, the first slot first, then jumps on: for a handler that
/// pushes arguments before going back into the game.
#[derive(Debug)]
pub(crate) struct PushTail {
    code: usize,
    slots: &'static [AtomicU32],
}

impl PushTail {
    /// A tail of `count` pushes that goes on at `to`, kept for the life of the process.
    pub(crate) fn new(count: usize, to: usize) -> Result<&'static Self, String> {
        let slots: &'static [AtomicU32] =
            Box::leak((0..count).map(|_| AtomicU32::new(0)).collect());
        let code = exec::place(count * 6 + 5, |at| {
            let mut code = Vec::with_capacity(count * 6 + 5);
            for slot in slots {
                // push dword [slot]
                code.extend_from_slice(&[0xFF, 0x35]);
                code.extend_from_slice(&(slot.as_ptr() as u32).to_le_bytes());
            }
            code.extend_from_slice(&patch::jmp_rel32(at + code.len(), to));
            Ok::<_, String>(code)
        })?;
        Ok(Box::leak(Box::new(Self { code, slots })))
    }

    /// Makes the hook go on through the tail, which pushes `values` in order. The game runs on one thread and the
    /// tail runs right after the handler, so the slots are free again by the next hit.
    pub(crate) fn resume(&self, ctx: &mut Context, values: &[u32]) {
        for (slot, v) in self.slots.iter().zip(values) {
            slot.store(*v, Ordering::Relaxed);
        }
        ctx.resume = self.code;
    }
}

/// Whether the C string at `text` equals `name`, as the game's `_mbscmp` answers 0 (equal bytes).
///
/// # Safety
/// `text` must point at a NUL-terminated string, as the original assumes of the register it passes.
pub(crate) unsafe fn c_str_is(text: u32, name: &[u8]) -> bool {
    // SAFETY: the caller guarantees the string.
    let s = unsafe { std::ffi::CStr::from_ptr(text as usize as *const std::ffi::c_char) };
    s.to_bytes() == name
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn st0_is_read_and_replaced_in_place() {
        let mut out = 0f32;
        let x = 2.5f32;
        // SAFETY: pushes one value, which the last block pops again.
        unsafe {
            core::arch::asm!("fld dword ptr [{p}]", p = in(reg) &raw const x, options(nostack))
        };
        let seen = st0();
        set_st0(seen * 3.0);
        // SAFETY: pops the value pushed above.
        unsafe {
            core::arch::asm!("fstp dword ptr [{p}]", p = in(reg) &raw mut out, options(nostack))
        };
        assert_eq!(seen, 2.5);
        assert_eq!(out, 7.5);
    }

    #[test]
    fn an_entry_stub_is_a_jump_target() {
        extern "C" fn handler(_: usize, _: &mut hook::Context) {}
        let at = entry(handler, 0).unwrap();
        assert!(at > 0x10000, "{at:#x}");
        assert_eq!(rel32(0x1000, 0x1010), [0x10, 0, 0, 0]);
        assert_eq!(rel32(0x1010, 0x1000), [0xF0, 0xFF, 0xFF, 0xFF]);
    }
}
