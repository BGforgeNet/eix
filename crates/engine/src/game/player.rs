//! Players: `CPlayer` (the part the addon reads), its party records, its units and its named float variables.
//!
//! Names follow ei-rebuild.
#![expect(
    unsafe_code,
    reason = "walking game.exe's player tables and calling its variable setter"
)]

use core::ffi::{CStr, c_char, c_void};
use core::mem::offset_of;

use eix_sites::Site;

use super::item::Item;
use super::object::EngineObject;
use super::stl::{HashNode, HashTable};
use super::unit::{Unit, UnitStats};
use super::world::PlayerManager;
use super::{Global, code};

/// The head of `CPlayer` up to the fields the addon reads; never held by value.
#[repr(C)]
#[derive(Debug)]
pub struct Player {
    _unknown0: [u8; 0x20],
    /// The server-side half, or null on a client: `CWorldServer::AwardExperience` 0x5911C0 sends through it.
    pub server: *mut PlayerServer,
    /// The player's number: `CWorldServer::GetPlayer` 0x594180 compares it with the number asked for.
    pub number: i32,
    _unknown28: [u8; 0x40 - 0x28],
    /// The player's units by id: `CPlayerClient::OnExperienceAll` 0x66A4AD walks it, taking each node's unit.
    pub units: HashTable<UnitMapNode>,
    _unknown54: [u8; 0xA0 - 0x54],
    /// The party: the party lookup 0x6621B0 (called on +0x70) walks `party_count` records from here.
    pub party: *mut PartyRecord,
    /// How many [`party`](Self::party) records there are (0x6621B0).
    pub party_count: i32,
    _unknowna8: [u8; 0xC4 - 0xA8],
    /// The player's script variables: `NamedFloatTable::Set` 0x460BC0 is called on +0xC4.
    pub variables: NamedFloatTable,
    _unknownd8: [u8; 0x108 - 0xD8],
    /// Non-zero while the player is invulnerable (god mode): `CUnitServer::TakeHit` 0x554146 ignores the hit.
    pub god_mode: u8,
}

const _: () = {
    assert!(offset_of!(Player, server) == 0x20);
    assert!(offset_of!(Player, number) == 0x24);
    assert!(offset_of!(Player, units) == 0x40);
    assert!(offset_of!(Player, party) == 0xA0);
    assert!(offset_of!(Player, party_count) == 0xA4);
    assert!(offset_of!(Player, variables) == 0xC4);
    assert!(offset_of!(Player, god_mode) == 0x108);
};

/// The client half of a player (`CPlayerClient`) up to the player it belongs to.
#[repr(C)]
#[derive(Debug)]
pub struct PlayerClient {
    /// The engine object.
    pub base: EngineObject,
    /// The player: `CPlayerClient::OnExperienceAll` 0x66A4AD reads its units through +0x1C.
    pub player: *mut Player,
}

const _: () = {
    assert!(offset_of!(PlayerClient, player) == 0x1C);
};

/// A node of [`Player::units`].
#[repr(C)]
#[derive(Debug)]
pub struct UnitMapNode {
    /// The next node of the bucket.
    pub next: *mut UnitMapNode,
    /// The key: the unit's id.
    pub id: i32,
    /// The unit: `OnExperienceAll` 0x66A4EE reads +8.
    pub unit: *mut Unit,
}

const _: () = {
    assert!(offset_of!(UnitMapNode, next) == 0x0);
    assert!(offset_of!(UnitMapNode, id) == 0x4);
    assert!(offset_of!(UnitMapNode, unit) == 0x8);
};

// SAFETY: `#[repr(C)]`, `next` first.
unsafe impl HashNode for UnitMapNode {}

/// A game vector of item pointers: the pointer and the count, as `CUnit::GetWeapon` 0x5318B0 reads them.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemList {
    /// The first item pointer.
    pub items: *mut *mut Item,
    /// How many there are (0x5318B0 checks the slot against it, unsigned).
    pub count: u32,
    _unknown8: [u8; 8],
}

const _: () = {
    assert!(size_of::<ItemList>() == 0x10);
};

/// One hero of a player's party, 0x784 bytes (the party lookup 0x6621B0 steps by it).
#[repr(C)]
#[derive(Debug)]
pub struct PartyRecord {
    _unknown0: [u8; 0x1C],
    /// The id of the hero's unit: 0x6621B0 matches it.
    pub unit_id: i32,
    /// The hero's statistics: `CCampScreen::BestPartySkill` 0x610020 reads each record's at +0x20.
    pub stats: UnitStats,
    _unknown724: [u8; 0x730 - 0x724],
    /// The hero's weapons: `CUnit::GetWeapon` 0x5318B0 reads a player's unit's weapon here.
    pub weapons: ItemList,
    /// Quick items: `CUnit::GetQuickItem` 0x531920.
    pub quick_items: ItemList,
    /// Armour pieces.
    pub armor: ItemList,
    /// Spell items: `CUnit::GetSpell` 0x5319A0.
    pub spells: ItemList,
    _unknown770: [u8; 0x784 - 0x770],
}

const _: () = {
    assert!(offset_of!(PartyRecord, unit_id) == 0x1C);
    assert!(offset_of!(PartyRecord, stats) == 0x20);
    assert!(offset_of!(PartyRecord, weapons) == 0x730);
    assert!(offset_of!(PartyRecord, quick_items) == 0x740);
    assert!(offset_of!(PartyRecord, armor) == 0x750);
    assert!(offset_of!(PartyRecord, spells) == 0x760);
    assert!(size_of::<PartyRecord>() == 0x784);
};

/// The file name of the multiplayer hero in use, a counted C string (the characters' address): `1.mp` in a game the
/// first hero created. Only the addon's crash backup reads it, which names its party files for it.
pub const HERO_FILE: Global<*const c_char> = Global::at(eix_sites::DATA_7C8320);

impl Player {
    /// Writes the player's party into the memory stream `stream`: `CParty::SaveParty` 0x661C10 (thiscall, `ret 8`) on
    /// the party object at +0x70, which the party lookup 0x6621B0 and `SyncPartyRecords` 0x590C70 also take.
    /// `flag` is 1 as the addon passes it.
    ///
    /// # Safety
    /// `this` must be a live player of a supported game.exe, on the game's thread; `stream` a constructed memory stream.
    pub unsafe fn save_party(this: *mut Self, stream: *mut c_void, flag: u32) {
        let party = this.cast::<u8>().wrapping_add(0x70).cast::<c_void>();
        // SAFETY: 0x661C10 is `void __thiscall (ByteStream*, int)`.
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut c_void, *mut c_void, u32)>(
                eix_sites::C_PARTY_SAVE_PARTY,
            )
        };
        // SAFETY: the caller guarantees the player, the stream and the thread.
        unsafe { f(party, stream, flag) }
    }

    /// The party record of the unit with id `unit_id`, the first match, or null.
    ///
    /// # Safety
    /// `this` must be a live player of a supported game.exe, on the game's thread.
    pub unsafe fn party_record(this: *const Self, unit_id: i32) -> *mut PartyRecord {
        // SAFETY: the caller guarantees a live player.
        let player = unsafe { &*this };
        let (party, count) = (player.party, player.party_count);
        for i in 0..usize::try_from(count).unwrap_or(0) {
            // SAFETY: the player holds `count` records from `party`.
            let record = unsafe { party.add(i) };
            // SAFETY: as above.
            if unsafe { (*record).unit_id } == unit_id {
                return record;
            }
        }
        core::ptr::null_mut()
    }

    /// The local player, whose party the camp shows: `LocalPlayerData` 0x578BA0 (no arguments, plain `ret`) gives
    /// its party object at +0x70, the player of the client world's local view.
    ///
    /// # Safety
    /// The process must be a supported game.exe, on the game's thread, while the client world has a local view (a camp
    /// or hero screen is open).
    pub unsafe fn local() -> *mut Self {
        // SAFETY: 0x578BA0 sets ECX itself and takes nothing else.
        let f = unsafe { code::<unsafe extern "C" fn() -> *mut u8>(eix_sites::LOCAL_PLAYER_DATA) };
        // SAFETY: the caller guarantees the build, the thread and the local view.
        unsafe { f() }.wrapping_sub(0x70).cast()
    }
}

/// The game's walk over a player manager's list: the list (whose count at +4 it holds a reference on) and the next
/// node.
///
/// `ObjectListIterator::First` 0x404920 takes the reference and the list's first node (+0x38); a node holds the
/// next node at +0 and the player at +8.
#[repr(C)]
#[derive(Debug)]
pub struct PlayerIterator {
    list: *mut PlayerManager,
    node: *mut c_void,
}

const _: () = {
    assert!(size_of::<PlayerIterator>() == 8);
};

impl Default for PlayerIterator {
    fn default() -> Self {
        Self::new()
    }
}

impl PlayerIterator {
    /// An iterator over no list, as the game clears one before `First`.
    pub const fn new() -> Self {
        Self {
            list: core::ptr::null_mut(),
            node: core::ptr::null_mut(),
        }
    }

    /// Starts at `list` and returns its first player, or null: `ObjectListIterator::First` 0x404920 (thiscall,
    /// `ret 4`).
    ///
    /// # Safety
    /// The process must be a supported game.exe, on the game's thread; `list` must be a live player manager.
    pub unsafe fn first(&mut self, list: *mut PlayerManager) -> *mut Player {
        // SAFETY: 0x404920 is `CPlayer* __thiscall (list)`.
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut Self, *mut PlayerManager) -> *mut Player>(
                eix_sites::OBJECT_LIST_ITERATOR_FIRST,
            )
        };
        // SAFETY: the caller guarantees the build, the thread and the list.
        unsafe { f(self, list) }
    }

    /// The next player, or null at the end: `ObjectListIterator::Next` 0x404960 (thiscall, plain `ret`).
    ///
    /// # Safety
    /// As for [`first`](Self::first), after `first` was called on this iterator.
    pub unsafe fn next(&mut self) -> *mut Player {
        // SAFETY: 0x404960 is `CPlayer* __thiscall ()`.
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut Self) -> *mut Player>(
                eix_sites::OBJECT_LIST_ITERATOR_NEXT,
            )
        };
        // SAFETY: the caller guarantees the build, the thread and a started walk.
        unsafe { f(self) }
    }

    /// Drops the reference on the list: `DropObjectReference` 0x455C80 (thiscall, plain `ret`).
    ///
    /// # Safety
    /// As for [`first`](Self::first).
    pub unsafe fn release(&mut self) {
        // SAFETY: 0x455C80 is `void __thiscall ()`; it skips a null list.
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut Self)>(eix_sites::DROP_OBJECT_REFERENCE)
        };
        // SAFETY: the caller guarantees the build and the thread.
        unsafe { f(self) }
    }
}

/// A node of a [`NamedFloatTable`]: `NamedFloatTable::Set` 0x460BC0 follows `next` and compares the name at +4.
#[repr(C)]
#[derive(Debug)]
pub struct NamedFloat {
    /// The next node of the bucket.
    pub next: *mut NamedFloat,
    /// The variable's name, a C string.
    pub name: *const c_char,
    /// Its value.
    pub value: f32,
}

const _: () = {
    assert!(offset_of!(NamedFloat, next) == 0x0);
    assert!(offset_of!(NamedFloat, name) == 0x4);
    assert!(offset_of!(NamedFloat, value) == 0x8);
};

// SAFETY: `#[repr(C)]`, `next` first.
unsafe impl HashNode for NamedFloat {}

/// `NamedFloatTable`: an STLport `hash_map<CString, float>`.
pub type NamedFloatTable = HashTable<NamedFloat>;

impl HashTable<NamedFloat> {
    /// Calls `f` with each variable's name and value in the table's order.
    ///
    /// # Safety
    /// `this` must be a live table of a supported game.exe, on the game's thread; `f` must not change it.
    pub unsafe fn for_each_variable(this: *const Self, mut f: impl FnMut(&CStr, f32)) {
        let visit = |node: *mut NamedFloat| {
            // SAFETY: `for_each` passes live nodes of the table.
            let n = unsafe { &*node };
            // SAFETY: a node's name is a C string the table owns.
            f(unsafe { CStr::from_ptr(n.name) }, n.value);
        };
        // SAFETY: passed on; the closure reads each node only.
        unsafe { Self::for_each(this, visit) };
    }

    /// The value of `name` (case-sensitive, as the addon's `strcmp`), or `None`.
    ///
    /// # Safety
    /// As for [`for_each_variable`](Self::for_each_variable).
    pub unsafe fn get(this: *const Self, name: &CStr) -> Option<f32> {
        let mut found = None;
        // SAFETY: passed on; the closure does not change the table.
        unsafe {
            Self::for_each_variable(this, |n, v| {
                if found.is_none() && n == name {
                    found = Some(v);
                }
            });
        }
        found
    }

    /// Sets `name` to `value`; zero removes it: `NamedFloatTable::Set` 0x460BC0 (thiscall, `ret 8`).
    ///
    /// # Safety
    /// As for [`for_each_variable`](Self::for_each_variable).
    pub unsafe fn set(this: *mut Self, name: &CStr, value: f32) {
        // SAFETY: 0x460BC0 is `void __thiscall (const char* name, float value)`.
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut Self, *const c_char, f32)>(
                eix_sites::NAMED_FLOAT_TABLE_SET,
            )
        };
        // SAFETY: the caller guarantees the table and the thread; `name` is NUL-terminated.
        unsafe { f(this, name.as_ptr(), value) }
    }

    /// The value of `name`, 0 when missing, by the game's own lookup: `NamedFloatTable::Get` 0x460AF0 (thiscall,
    /// `ret 4`), which hashes the name and compares it in its bucket with `_stricmp` (0x460B4A), unlike
    /// [`get`](Self::get).
    ///
    /// # Safety
    /// As for [`for_each_variable`](Self::for_each_variable).
    pub unsafe fn lookup(this: *mut Self, name: &CStr) -> f32 {
        // SAFETY: 0x460AF0 is `float __thiscall (const char* name)`, the result in ST0.
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut Self, *const c_char) -> f32>(
                eix_sites::NAMED_FLOAT_TABLE_GET,
            )
        };
        // SAFETY: the caller guarantees the table and the thread; `name` is NUL-terminated.
        unsafe { f(this, name.as_ptr()) }
    }

    /// Empties the table, freeing every node: [`CLEAR`].
    ///
    /// # Safety
    /// As for [`for_each_variable`](Self::for_each_variable).
    pub unsafe fn clear(this: *mut Self) {
        // SAFETY: 0x461540 is `void __thiscall ()`.
        let f = unsafe { code::<unsafe extern "thiscall" fn(*mut Self)>(CLEAR) };
        // SAFETY: the caller guarantees the table and the thread.
        unsafe { f(this) }
    }
}

/// `CStrFloat::Clear` 0x461540 (thiscall, plain `ret`): frees every node of a [`NamedFloatTable`] and zeroes its count
/// (its +0x10).
pub const CLEAR: Site = eix_sites::C_STR_FLOAT_CLEAR;

/// `CPlayerServer`, the server's view of a player; opaque.
#[repr(C)]
#[derive(Debug)]
pub struct PlayerServer {
    _opaque: [u8; 0],
}

impl PlayerServer {
    /// Sets the player's variable `name` and sends it: `CPlayerServer::SetVariable` 0x66EE10 (thiscall, the name and
    /// the float on the stack, `ret 8`).
    ///
    /// # Safety
    /// The process must be a supported game.exe, on the game's thread; `this` must be a live player's server half.
    pub unsafe fn set_variable(this: *mut Self, name: &CStr, value: f32) {
        // SAFETY: 0x66EE10 is `void __thiscall (const char*, float)`.
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut Self, *const c_char, f32)>(
                eix_sites::C_PLAYER_SERVER_SET_VARIABLE,
            )
        };
        // SAFETY: the caller guarantees the player and the thread; `name` is NUL-terminated.
        unsafe { f(this, name.as_ptr(), value) }
    }

    /// Sends a unit's experience gain to the player's client: `CPlayerServer::SendPair18` 0x66F2C0 (thiscall, the
    /// unit id and the float on the stack, `ret 8`).
    ///
    /// # Safety
    /// As for [`set_variable`](Self::set_variable).
    pub unsafe fn send_unit_experience(this: *mut Self, unit_id: i32, value: f32) {
        // SAFETY: 0x66F2C0 is `void __thiscall (int, float)`.
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut Self, i32, f32)>(
                eix_sites::C_PLAYER_SERVER_SEND_PAIR_18,
            )
        };
        // SAFETY: as for `set_variable`.
        unsafe { f(this, unit_id, value) }
    }

    /// Sends the player's party again: `CPlayerServer::ResendParty` 0x66F450 (thiscall, plain `ret`).
    ///
    /// # Safety
    /// As for [`set_variable`](Self::set_variable).
    pub unsafe fn resend_party(this: *mut Self) {
        // SAFETY: 0x66F450 is `void __thiscall ()`.
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut Self)>(eix_sites::C_PLAYER_SERVER_RESEND_PARTY)
        };
        // SAFETY: as for `set_variable`.
        unsafe { f(this) }
    }

    /// Sends a string event to the player's client, which raises it there: `CPlayerServer::SendText13` 0x66D770
    /// (thiscall, 0 and the text on the stack, `ret 8`).
    ///
    /// # Safety
    /// As for [`set_variable`](Self::set_variable).
    pub unsafe fn send_string_event(this: *mut Self, text: &CStr) {
        // SAFETY: 0x66D770 is `void __thiscall (int, const char*)`; the addon passes 0 first.
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut Self, i32, *const c_char)>(
                eix_sites::C_PLAYER_SERVER_SEND_TEXT_13,
            )
        };
        // SAFETY: as for `set_variable`; `text` is NUL-terminated.
        unsafe { f(this, 0, text.as_ptr()) }
    }
}
