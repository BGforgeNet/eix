//! The worlds: `CWorld`, the base of the server's world and the client's, and the two static instances.
//!
//! Names follow ei-rebuild.
#![expect(unsafe_code, reason = "calls into game.exe's world code")]

use core::ffi::CStr;
use core::mem::offset_of;

use super::item::Item;
use super::mfc::{GameString, PtrList};
use super::object::{EngineObject, Vector3};
use super::player::Player;
use super::unit::{MapObject, Unit};
use super::{Global, code};

/// The head of `CWorld`, which `CWorldServer` and `CWorldClient` extend; never held by value.
///
/// `CWorld::ConstructBase` 0x55EAB0 clears the engine object's fields, then the world's.
#[repr(C)]
#[derive(Debug)]
pub struct World {
    /// The engine object.
    pub base: EngineObject,
    /// The game time at tick 0: `CWorld::SetTimeImpl` 0x55FB30 sets it so that the time now is the one asked for.
    pub time_base: f32,
    /// Game time per tick: `CWorld::GetTime` 0x55FBD0 multiplies the tick by it.
    pub time_per_tick: f32,
    _unknown24: [u8; 0x64 - 0x24],
    /// The world's tick (0x55FBD0, 0x55FB30).
    pub tick: i32,
    _unknown68: [u8; 0x6C - 0x68],
    /// Non-zero while the game is paused: `CWorld::SetPaused` 0x566350.
    pub paused: u8,
}

const _: () = {
    assert!(offset_of!(World, time_base) == 0x1C);
    assert!(offset_of!(World, time_per_tick) == 0x20);
    assert!(offset_of!(World, tick) == 0x64);
    assert!(offset_of!(World, paused) == 0x6C);
};

/// The server's world (`CWorldServer`), constructed in place by its static initialiser (0x589BE5 calls
/// `CWorldServer::Construct` 0x589ED0).
pub const SERVER_WORLD: Global<World> = Global::at(eix_sites::DATA_7AFF60);
/// The client's world (`CWorldClient`), constructed in place by its static initialiser (0x560625 calls
/// `CWorldClient::Construct` 0x5612F0).
pub const CLIENT_WORLD: Global<World> = Global::at(eix_sites::DATA_7AF5B8);

/// The head of the world's player manager, up to its player count.
#[repr(C)]
#[derive(Debug)]
pub struct PlayerManager {
    _unknown0: [u8; 0x2C],
    /// The number of players: `CWorldServer::AwardExperience` awards nothing while it is 0 (0x591209).
    pub player_count: i32,
}

const _: () = {
    assert!(offset_of!(PlayerManager, player_count) == 0x2C);
};

impl World {
    /// The world's player manager: virtual slot 20 (vtable +0x50), which `CWorldServer::GetPlayer` 0x594180 and
    /// `AwardExperience` (0x591206) call.
    ///
    /// # Safety
    /// `this` must be one of the game's worlds, constructed, on the game's thread.
    pub unsafe fn players(this: *mut Self) -> *mut PlayerManager {
        // SAFETY: a constructed world's first word is its vtable.
        let vtable = unsafe { (*this).base.vtable }.cast::<usize>();
        // SAFETY: the world's vtable has slot 20.
        let slot = unsafe { vtable.wrapping_add(20).read() };
        // SAFETY: slot 20 is `CPlayerManager* __thiscall ()` (0x402CE0 for the server's world).
        let f = unsafe {
            crate::hook::as_fn_at::<unsafe extern "thiscall" fn(*mut Self) -> *mut PlayerManager>(
                slot,
            )
        };
        // SAFETY: the caller guarantees the world and the thread.
        unsafe { f(this) }
    }

    /// The player numbered `number`, or null: `CWorldServer::GetPlayer` 0x594180 (thiscall, `ret 4`).
    ///
    /// # Safety
    /// `this` must be [`SERVER_WORLD`], constructed, on the game's thread.
    pub unsafe fn get_player(this: *mut Self, number: i32) -> *mut Player {
        // SAFETY: 0x594180 is `CPlayer* __thiscall (int number)`.
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut Self, i32) -> *mut Player>(
                eix_sites::C_WORLD_SERVER_GET_PLAYER,
            )
        };
        // SAFETY: the caller guarantees the world and the thread.
        unsafe { f(this, number) }
    }

    /// Floats `value` over the object with id `id`, in the style `kind` (8 for experience):
    /// `CWorldClient::ShowNumber` 0x56CE40 (thiscall, `ret 0xC`), as `CPlayerClient::OnExperience` calls it
    /// (0x66A612).
    ///
    /// # Safety
    /// `this` must be [`CLIENT_WORLD`], constructed, on the game's thread.
    pub unsafe fn show_number(this: *mut Self, id: i32, value: i32, kind: i32) {
        // SAFETY: 0x56CE40 is `void __thiscall (int id, int value, int kind)`.
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut Self, i32, i32, i32)>(
                eix_sites::C_WORLD_CLIENT_SHOW_NUMBER,
            )
        };
        // SAFETY: the caller guarantees the world and the thread.
        unsafe { f(this, id, value, kind) }
    }

    /// Pauses (`true`) or resumes the world: `CWorld::SetPaused` 0x566350 (thiscall, `ret 4`).
    ///
    /// # Safety
    /// `this` must be one of the game's worlds, constructed, on the game's thread.
    pub unsafe fn set_paused(this: *mut Self, paused: bool) {
        // SAFETY: 0x566350 is `void __thiscall (bool)`: it stores the low byte of its one stack argument.
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut Self, u32)>(eix_sites::C_WORLD_SET_PAUSED)
        };
        // SAFETY: the caller guarantees the world and the thread.
        unsafe { f(this, u32::from(paused)) }
    }

    /// The map object with id `id`, or null: `CWorld::FindMapObject` 0x55FAE0 (thiscall, `ret 4`).
    ///
    /// # Safety
    /// As for [`set_paused`](Self::set_paused); the world's managers must exist (a zone is loaded).
    pub unsafe fn find_map_object(this: *mut Self, id: i32) -> *mut MapObject {
        // SAFETY: 0x55FAE0 is `CMapObject* __thiscall (int id)`.
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut Self, i32) -> *mut MapObject>(
                eix_sites::C_WORLD_FIND_MAP_OBJECT,
            )
        };
        // SAFETY: the caller guarantees the world, its managers and the thread.
        unsafe { f(this, id) }
    }

    /// The server world's object with id `id`, of any kind, or null: `CWorldServer::FindObject` 0x593AA0 (thiscall,
    /// `ret 4`), a lookup in the world's id table.
    ///
    /// # Safety
    /// `this` must be [`SERVER_WORLD`], constructed, on the game's thread.
    pub unsafe fn find_object(this: *mut Self, id: i32) -> *mut EngineObject {
        // SAFETY: 0x593AA0 is `CEngineObject* __thiscall (int id)`.
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut Self, i32) -> *mut EngineObject>(
                eix_sites::C_WORLD_SERVER_FIND_OBJECT,
            )
        };
        // SAFETY: the caller guarantees the world and the thread.
        unsafe { f(this, id) }
    }

    /// The unit with id `id`, or null when there is none or the object is no unit: `CWorld::FindUnit` 0x55FB00
    /// (thiscall, `ret 4`).
    ///
    /// # Safety
    /// As for [`find_map_object`](Self::find_map_object).
    pub unsafe fn find_unit(this: *mut Self, id: i32) -> *mut Unit {
        // SAFETY: 0x55FB00 is `CUnit* __thiscall (int id)`.
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut Self, i32) -> *mut Unit>(
                eix_sites::C_WORLD_FIND_UNIT,
            )
        };
        // SAFETY: as for `find_map_object`.
        unsafe { f(this, id) }
    }
}

/// The server world's switch for its first script queue: `CWorldServer::Update` 0x58DFF0's script step (0x58DF87)
/// runs the zone's script only while it is non-zero (its +0x160).
pub const SCRIPTS_ACTIVE: Global<i32> = Global::at(eix_sites::DATA_7B00C0);

/// The server world's map of all its objects by id (its +0x12D70): `CWorldServer::CreateObject` 0x592A8F enters
/// each new object. Its nodes are laid out as [`SERVER_OBJECTS`]'.
pub const OBJECT_MAP: Global<ObjectMap> = Global::at(eix_sites::DATA_7C2CD0);

/// The server world's sector map (its +0x294), which object placement asks (`UnitSpecApply` 0x46C4ED).
pub const SECTOR_MAP: Global<SectorMap> = Global::at(eix_sites::DATA_7B01F4);

/// `CSectorMap`; opaque.
#[repr(C)]
#[derive(Debug)]
pub struct SectorMap {
    _opaque: [u8; 0],
}

impl SectorMap {
    /// Appends the objects within `radius` of `point` to `list`: `CSectorMap::FindUnitsInRadius` 0x5C0BD0 (thiscall,
    /// the point, the list and the radius on the stack, `ret 0xC`).
    ///
    /// # Safety
    /// The process must be a supported game.exe, on the game's thread, with a zone loaded; `list` must be a constructed
    /// list.
    pub unsafe fn find_in_radius(
        this: *mut Self,
        point: &Vector3,
        list: *mut PtrList,
        radius: f32,
    ) {
        // SAFETY: 0x5C0BD0 is `void __thiscall (const Vector3*, CPtrList*, float)`.
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut Self, *const Vector3, *mut PtrList, f32)>(
                eix_sites::C_SECTOR_MAP_FIND_UNITS_IN_RADIUS,
            )
        };
        // SAFETY: the caller guarantees the map, the list and the thread.
        unsafe { f(this, point, list, radius) }
    }
}

/// The server world's own calls; `this` is always [`SERVER_WORLD`].
impl World {
    /// A new item made from its database name, or null: `CWorldServer::CreateItemNamed` 0x594AB0 (thiscall, `ret 4`).
    ///
    /// # Safety
    /// `this` must be the server world, constructed, on the game's thread, with a zone loaded.
    pub unsafe fn create_item(this: *mut Self, name: &CStr) -> *mut Item {
        // SAFETY: 0x594AB0 is `CItem* __thiscall (const char*)`.
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut Self, *const u8) -> *mut Item>(
                eix_sites::C_WORLD_SERVER_CREATE_ITEM_NAMED,
            )
        };
        // SAFETY: as for `get_player`; `name` is NUL-terminated.
        unsafe { f(this, name.as_ptr().cast()) }
    }

    /// Awards `value` experience to `player` (null: the network path): `CWorldServer::AwardExperience` 0x5911C0
    /// (thiscall, `ret 8`).
    ///
    /// # Safety
    /// As for [`create_item`](Self::create_item); `player` null or a live player.
    pub unsafe fn award_experience(this: *mut Self, player: *mut Player, value: f32) {
        // SAFETY: 0x5911C0 is `void __thiscall (CPlayer*, float)`; it tests the player for null (0x5911FC).
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut Self, *mut Player, f32)>(
                eix_sites::C_WORLD_SERVER_AWARD_EXPERIENCE,
            )
        };
        // SAFETY: as for `create_item`.
        unsafe { f(this, player, value) }
    }

    /// `player` takes the dead `unit`'s items: `CWorldServer::Loot` 0x5942D0 (thiscall, `ret 8`).
    ///
    /// # Safety
    /// As for [`create_item`](Self::create_item); `unit` and `player` live.
    pub unsafe fn loot(this: *mut Self, unit: *mut Unit, player: *mut Player) {
        // SAFETY: 0x5942D0 is `void __thiscall (CUnit*, CPlayer*)`.
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut Self, *mut Unit, *mut Player)>(
                eix_sites::C_WORLD_SERVER_LOOT,
            )
        };
        // SAFETY: as for `create_item`.
        unsafe { f(this, unit, player) }
    }

    /// Removes the object with id `id`: `CWorldServer::RemoveObject` 0x5935C0 (thiscall, `ret 4`).
    ///
    /// # Safety
    /// As for [`create_item`](Self::create_item).
    pub unsafe fn remove_object(this: *mut Self, id: i32) {
        // SAFETY: 0x5935C0 is `void __thiscall (int)`.
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut Self, i32)>(
                eix_sites::C_WORLD_SERVER_REMOVE_OBJECT,
            )
        };
        // SAFETY: as for `create_item`.
        unsafe { f(this, id) }
    }

    /// Loads a .mob file's objects (with `skip_scripts`, not its scripts); 0 on failure:
    /// `CWorldServer::LoadMapObjects` 0x596030 (thiscall, `ret 8`).
    ///
    /// # Safety
    /// As for [`create_item`](Self::create_item).
    pub unsafe fn load_map_objects(this: *mut Self, file: &CStr, skip_scripts: bool) -> i32 {
        // SAFETY: 0x596030 is `int __thiscall (const char*, int)`; the mob reader tests the flag's low byte.
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut Self, *const u8, u32) -> i32>(
                eix_sites::C_WORLD_SERVER_LOAD_MAP_OBJECTS,
            )
        };
        // SAFETY: as for `create_item`; `file` is NUL-terminated.
        unsafe { f(this, file.as_ptr().cast(), u32::from(skip_scripts)) }
    }

    /// Sets the weather `kind` (0 clear, 1 rain, 2 snow) at `strength`: `CWorldServer::SetWeather` 0x58EE00
    /// (thiscall, `ret 8`).
    ///
    /// # Safety
    /// As for [`create_item`](Self::create_item).
    pub unsafe fn set_weather(this: *mut Self, kind: i32, strength: f32) {
        // SAFETY: 0x58EE00 is `void __thiscall (int, float)`.
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut Self, i32, f32)>(
                eix_sites::C_WORLD_SERVER_SET_WEATHER,
            )
        };
        // SAFETY: as for `create_item`.
        unsafe { f(this, kind, strength) }
    }

    /// Loads the scripts of mob file `path` into the zone's script ([`MISSION_SCRIPT`](super::script::MISSION_SCRIPT)):
    /// `CWorldServer::LoadZoneScripts` 0x596350 (thiscall, `ret 4`). The addon's `check` calls it before any zone is
    /// loaded.
    ///
    /// # Safety
    /// `this` must be the server world, constructed, on the game's thread.
    pub unsafe fn load_zone_scripts(this: *mut Self, path: &CStr) -> i32 {
        // SAFETY: 0x596350 is `int __thiscall (const char*)`.
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut Self, *const u8) -> i32>(
                eix_sites::C_WORLD_SERVER_LOAD_ZONE_SCRIPTS,
            )
        };
        // SAFETY: the caller guarantees the world and the thread; `path` is NUL-terminated.
        unsafe { f(this, path.as_ptr().cast()) }
    }

    /// Sets the server world up for a zone: `CWorldServer::Setup` 0x58AE10 (thiscall, plain `ret`).
    ///
    /// # Safety
    /// As for [`load_zone_scripts`](Self::load_zone_scripts).
    pub unsafe fn setup(this: *mut Self) {
        // SAFETY: 0x58AE10 is `void __thiscall ()`.
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut Self)>(eix_sites::C_WORLD_SERVER_SETUP)
        };
        // SAFETY: the caller guarantees the world and the thread.
        unsafe { f(this) }
    }

    /// Loads zone `map` (`.mpr`) with mob `mob` and zone name `zone`; 0 on failure: `CWorldServer::NewZone` 0x595480
    /// (thiscall, `ret 0xC`).
    ///
    /// # Safety
    /// As for [`load_zone_scripts`](Self::load_zone_scripts).
    pub unsafe fn new_zone(this: *mut Self, map: &CStr, mob: &CStr, zone: &CStr) -> i32 {
        // SAFETY: 0x595480 is `int __thiscall (const char*, const char*, const char*)`.
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut Self, *const u8, *const u8, *const u8) -> i32>(
                eix_sites::C_WORLD_SERVER_NEW_ZONE,
            )
        };
        // SAFETY: the caller guarantees the world and the thread; the strings are NUL-terminated.
        unsafe {
            f(
                this,
                map.as_ptr().cast(),
                mob.as_ptr().cast(),
                zone.as_ptr().cast(),
            )
        }
    }

    /// Removes every unit of the zone: `CWorldServer::RemoveAllUnits` 0x5939E0 (thiscall, plain `ret`).
    ///
    /// # Safety
    /// As for [`load_zone_scripts`](Self::load_zone_scripts).
    pub unsafe fn remove_all_units(this: *mut Self) {
        // SAFETY: 0x5939E0 is `void __thiscall ()`.
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut Self)>(
                eix_sites::C_WORLD_SERVER_REMOVE_ALL_UNITS,
            )
        };
        // SAFETY: the caller guarantees the world and the thread.
        unsafe { f(this) }
    }

    /// Refreshes `player`'s party records: `CWorldServer::SyncPartyRecords` 0x590C70 (ei-rebuild's name; thiscall,
    /// `ret 4`); it walks the list at the player's +0xE4, then its `party_count` records.
    ///
    /// # Safety
    /// As for [`load_zone_scripts`](Self::load_zone_scripts); `player` a live player.
    pub unsafe fn sync_party_records(this: *mut Self, player: *mut Player) {
        // SAFETY: 0x590C70 is `void __thiscall (CPlayer*)`.
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut Self, *mut Player)>(
                eix_sites::C_WORLD_SERVER_SYNC_PARTY_RECORDS,
            )
        };
        // SAFETY: the caller guarantees the world, the player and the thread.
        unsafe { f(this, player) }
    }

    /// While the count at `player`'s +0xB4 is non-zero, removes the first entry of the list at +0xB0 through the
    /// party's remove (0x661F30, on +0x70): `CWorldServer::ClearPlayerItems` 0x591070 (ei-rebuild's name; thiscall,
    /// `ret 4`).
    ///
    /// # Safety
    /// As for [`sync_party_records`](Self::sync_party_records).
    pub unsafe fn clear_player_items(this: *mut Self, player: *mut Player) {
        // SAFETY: 0x591070 is `void __thiscall (CPlayer*)`.
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut Self, *mut Player)>(
                eix_sites::C_WORLD_SERVER_CLEAR_PLAYER_ITEMS,
            )
        };
        // SAFETY: the caller guarantees the world, the player and the thread.
        unsafe { f(this, player) }
    }

    /// Gathers the items of the zone's dead units: `CWorldServer::LootDeadUnits` 0x593D60 (ei-rebuild's name; thiscall,
    /// plain `ret`).
    ///
    /// # Safety
    /// As for [`load_zone_scripts`](Self::load_zone_scripts).
    pub unsafe fn loot_dead_units(this: *mut Self) {
        // SAFETY: 0x593D60 is `void __thiscall ()`.
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut Self)>(
                eix_sites::C_WORLD_SERVER_LOOT_DEAD_UNITS,
            )
        };
        // SAFETY: the caller guarantees the world and the thread.
        unsafe { f(this) }
    }

    /// The path the game finds mob file `mob` at, into the constructed string `out`: `MapPath` 0x595BA0 (thiscall,
    /// `ret 8`).
    ///
    /// # Safety
    /// As for [`load_zone_scripts`](Self::load_zone_scripts); `out` must be a constructed game string.
    pub unsafe fn map_path(this: *mut Self, out: *mut GameString, mob: &CStr) {
        // SAFETY: 0x595BA0 is `void __thiscall (CString*, const char*)`; its result is not used.
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut Self, *mut GameString, *const u8) -> usize>(
                eix_sites::MAP_PATH,
            )
        };
        // SAFETY: the caller guarantees the world, the string and the thread; `mob` is NUL-terminated.
        unsafe { f(this, out, mob.as_ptr().cast()) };
    }

    /// Asks the server to pause (non-zero) or resume: `CWorldClient::SendFlag10` 0x566330 (thiscall, `ret 4`).
    ///
    /// # Safety
    /// `this` must be the client world, constructed, with its session, on the game's thread.
    pub unsafe fn send_pause(this: *mut Self, paused: u32) {
        // SAFETY: 0x566330 is `void __thiscall (int)`.
        let f = unsafe {
            code::<unsafe extern "thiscall" fn(*mut Self, u32)>(
                eix_sites::C_WORLD_CLIENT_SEND_FLAG_10,
            )
        };
        // SAFETY: the caller guarantees the world and the thread.
        unsafe { f(this, paused) }
    }
}

/// The save version (`kSaveVersion`, ei-rebuild's name): the addon passes its value to `SaveFileWrite` 0x579B80.
pub const SAVE_VERSION: Global<u32> = Global::at(eix_sites::K_SAVE_VERSION);

/// The flag the loading movie checks (`ShowLoadingMovie` 0x5DA42C): `CWorldServer::LoadMobFile` 0x595AE9 sets it
/// while it loads, so no movie shows.
pub const LOADING_MOVIE_OFF: Global<u8> = Global::at(eix_sites::G_LOADING_MOVIE_OFF);

/// A node of an [`ObjectMap`] chain.
#[repr(C)]
#[derive(Debug)]
pub struct ObjectNode {
    /// The next node of the bucket, or null: the map's iterator step 0x59B7D0 follows it first.
    pub next: *mut ObjectNode,
    /// The object's id: the step hashes it to find the next bucket.
    pub id: u32,
    /// The object: `CWorldServer::UpdateUnits` 0x58D670 reads it from +8 (0x58D7AA).
    pub object: *mut MapObject,
}

const _: () = {
    assert!(offset_of!(ObjectNode, next) == 0x0);
    assert!(offset_of!(ObjectNode, id) == 0x4);
    assert!(offset_of!(ObjectNode, object) == 0x8);
};

/// The server's map of its map objects by id, a hash table of chains; never held by value.
///
/// The iterator's start 0x59C020 walks the bucket array (+4 to +8) to the first non-empty bucket.
#[repr(C)]
#[derive(Debug)]
pub struct ObjectMap {
    _unknown0: [u8; 4],
    /// The first bucket, a chain's first node or null.
    pub buckets: *mut *mut ObjectNode,
    /// One past the last bucket.
    pub buckets_end: *mut *mut ObjectNode,
}

const _: () = {
    assert!(offset_of!(ObjectMap, buckets) == 0x4);
    assert!(offset_of!(ObjectMap, buckets_end) == 0x8);
};

/// The server world's map objects (`CWorldServer` +0x12DA0: `UpdateUnits` 0x58D76B walks it).
pub const SERVER_OBJECTS: Global<ObjectMap> = Global::at(eix_sites::DATA_7AFF60.offset(0x1_2DA0));

impl ObjectMap {
    /// The nodes in bucket order, each bucket's chain in turn, as the game's iterator (0x59C020, 0x59B7D0) visits
    /// them. Each step reads the map, so the caller must not change it while it walks.
    ///
    /// # Safety
    /// `this` must be one of the game's constructed object maps, on the game's thread, unchanged during the walk.
    pub unsafe fn nodes(this: *const Self) -> ObjectNodes {
        // SAFETY: the caller guarantees a constructed map.
        let map = unsafe { &*this };
        ObjectNodes {
            bucket: map.buckets,
            end: map.buckets_end,
            node: core::ptr::null_mut(),
        }
    }
}

/// The walk of [`ObjectMap::nodes`].
#[derive(Debug)]
pub struct ObjectNodes {
    bucket: *mut *mut ObjectNode,
    end: *mut *mut ObjectNode,
    node: *mut ObjectNode,
}

impl Iterator for ObjectNodes {
    type Item = *mut ObjectNode;

    fn next(&mut self) -> Option<Self::Item> {
        if !self.node.is_null() {
            // SAFETY: a node the walk returned, which the map still holds (`nodes`' contract).
            self.node = unsafe { (*self.node).next };
            if !self.node.is_null() {
                return Some(self.node);
            }
            // SAFETY: the bucket the last node came from lies inside the array.
            self.bucket = unsafe { self.bucket.add(1) };
        }
        while self.bucket < self.end {
            // SAFETY: a bucket inside the array.
            let head = unsafe { *self.bucket };
            if !head.is_null() {
                self.node = head;
                return Some(head);
            }
            // SAFETY: still inside the array or one past it.
            self.bucket = unsafe { self.bucket.add(1) };
        }
        None
    }
}

/// Puts `object` into the server's update map, so it is updated again: `CWorldServer::AddToUpdateMap` 0x58D5D0
/// (thiscall, `ret 4`).
///
/// # Safety
/// The server world must be constructed, `object` a live map object of it, on the game's thread.
pub unsafe fn add_to_update_map(object: *mut MapObject) {
    // SAFETY: 0x58D5D0 is `void __thiscall CWorldServer::(CMapObject*)`.
    let f = unsafe {
        code::<unsafe extern "thiscall" fn(*mut World, *mut MapObject)>(
            eix_sites::C_WORLD_SERVER_ADD_TO_UPDATE_MAP,
        )
    };
    // SAFETY: the caller guarantees the world, the object and the thread.
    unsafe { f(SERVER_WORLD.as_ptr(), object) }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Universal Mod's notes name 0x7AF61C the game tick and 0x7AF624 the pause flag: the client world's fields.
    #[test]
    fn the_client_worlds_fields_are_the_known_globals() {
        assert_eq!(
            CLIENT_WORLD.site().offset(offset_of!(World, tick)),
            eix_sites::DATA_7AF61C
        );
        assert_eq!(
            CLIENT_WORLD.site().offset(offset_of!(World, paused)),
            eix_sites::DATA_7AF624
        );
    }

    #[test]
    fn the_walk_visits_each_bucket_and_its_chain_in_order() {
        let null: *mut ObjectNode = core::ptr::null_mut();
        let node = |id, next| ObjectNode {
            next,
            id,
            object: null.cast(),
        };
        let mut third = node(3, null);
        let mut second = node(2, null);
        let mut first = node(1, &raw mut second);
        let mut buckets = [null, &raw mut first, null, null, &raw mut third, null];
        let range = buckets.as_mut_ptr_range();
        let map = ObjectMap {
            _unknown0: [0; 4],
            buckets: range.start,
            buckets_end: range.end,
        };
        // SAFETY: a map of this test's own nodes, unchanged during the walk.
        let ids: alloc::vec::Vec<u32> = unsafe { ObjectMap::nodes(&map) }
            // SAFETY: each node the walk returns is one of the locals above.
            .map(|n| unsafe { (*n).id })
            .collect();
        assert_eq!(ids, [1, 2, 3]);
        let empty = ObjectMap {
            _unknown0: [0; 4],
            buckets: null.cast(),
            buckets_end: null.cast(),
        };
        // SAFETY: an empty map.
        assert_eq!(unsafe { ObjectMap::nodes(&empty) }.count(), 0);
    }
}
