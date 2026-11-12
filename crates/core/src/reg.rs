//! Nival's binary registry format (`.reg`): the mod's `config.reg` and `ai.reg`, `music.reg` and so on.
//!
//! Layout, little endian:
//! - header: magic `FB 3E AB 45`, `u16` section count N, then a hash table of N 6-byte slots (`u16` next slot in the
//!   chain, 0xFFFF at its end; `u32` absolute offset of the section);
//! - section: `u16` key count M, `u16` name length, the name, a hash table of M slots (same shape, key offsets relative
//!   to the section start), then the keys;
//! - key: a type byte (0 int, 1 float, 2 string, plus 0x80 for an array), `u16` name length, the name, for an array a
//!   `u16` element count, then the values: `i32`, `f32`, or `u16` length plus bytes.
//!
//! The table is a coalesced hash table: a name's home slot is the sum of its lowercased bytes (as signed `char`)
//! modulo the slot count, and a lookup follows the chain from there. The sections and keys themselves are stored in
//! file order, which is the order of their offsets; the reader takes that order and ignores the chains, and the writer
//! rebuilds them, so a file the game can look up is written for any [`Registry`].
//!
//! Strings are kept as raw bytes: the game reads them in the ANSI code page of the machine, and the format does not
//! say which. Lookups ignore ASCII case for section and key names, as the addon's do.

use std::fmt;

use crate::num::trunc_to_i32_wrapping;

/// The four bytes every `.reg` file starts with.
pub const MAGIC: [u8; 4] = [0xFB, 0x3E, 0xAB, 0x45];

/// The `next` value that ends a chain.
const CHAIN_END: u16 = 0xFFFF;
const ARRAY_FLAG: u8 = 0x80;
const TYPE_INT: u8 = 0;
const TYPE_FLOAT: u8 = 1;
const TYPE_STR: u8 = 2;

/// A value of one key.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// A single 32-bit integer.
    Int(i32),
    /// A single 32-bit float.
    Float(f32),
    /// A single string (raw bytes).
    Str(Vec<u8>),
    /// An array of integers.
    IntArray(Vec<i32>),
    /// An array of floats.
    FloatArray(Vec<f32>),
    /// An array of strings.
    StrArray(Vec<Vec<u8>>),
}

/// One named key.
#[derive(Clone, Debug, PartialEq)]
pub struct Key {
    /// The key name (raw bytes).
    pub name: Vec<u8>,
    /// The key value.
    pub value: Value,
}

/// One section with its keys in file order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Section {
    /// The section name (raw bytes).
    pub name: Vec<u8>,
    /// The keys in file order.
    pub keys: Vec<Key>,
}

/// A whole file: sections in file order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Registry {
    /// The sections in file order.
    pub sections: Vec<Section>,
}

/// A malformed file: where the reader stopped and what it needed there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    /// Byte offset in the file.
    pub offset: usize,
    /// What the reader expected at that offset.
    pub expected: &'static str,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "malformed registry file at offset {}: expected {}",
            self.offset, self.expected
        )
    }
}

impl std::error::Error for ParseError {}

/// A registry that the format cannot hold.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WriteError {
    /// Which limit was exceeded.
    pub what: &'static str,
}

impl fmt::Display for WriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "cannot write registry file: {}", self.what)
    }
}

impl std::error::Error for WriteError {}

/// Whether the original's file loader takes `data` as a registry file: six bytes or more, starting with
/// [`MAGIC`]. It checks nothing else; the lookups read the tables by their offsets without a bounds check.
pub fn loads(data: &[u8]) -> bool {
    data.len() >= 6 && data[..4] == MAGIC
}

/// Why a file is not usable as a registry file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadError {
    /// The original's loader refuses it ([`loads`] is false: empty, too short, another magic), and with it the
    /// game, on the box of [`config_load_error`] or [`ai_load_error`].
    Unusable,
    /// It has the header but a body that [`Registry::parse`] rejects. The original reads such a file unchecked
    /// (garbage, or a crash on an offset outside the file).
    Malformed(ParseError),
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::Unusable => f.write_str("not a registry file (no header)"),
            LoadError::Malformed(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for LoadError {}

impl Registry {
    /// Reads a file as the addon needs it.
    ///
    /// # Errors
    /// [`LoadError::Unusable`] where the original stops on a box, [`LoadError::Malformed`] where it reads on.
    pub fn load(data: &[u8]) -> Result<Registry, LoadError> {
        if !loads(data) {
            return Err(LoadError::Unusable);
        }
        Registry::parse(data).map_err(LoadError::Malformed)
    }
}

/// The text of the box that stops the game when the mod's config.reg cannot be loaded.
pub fn config_load_error(path: &str) -> String {
    format!("Can't open file: {path}")
}

/// The text of the box that stops the game when the ai.reg that config.reg names cannot be loaded.
pub fn ai_load_error(path: &str) -> String {
    format!("Fail to load: {path}")
}

impl Value {
    /// The integer, also from a float, as the addon's integer reader accepts both. The float goes through the
    /// original's own conversion ([`trunc_to_i32_wrapping`]), which agrees with a plain truncation inside the `i32`
    /// range.
    pub fn as_int(&self) -> Option<i32> {
        match self {
            Value::Int(v) => Some(*v),
            Value::Float(v) => Some(trunc_to_i32_wrapping(*v)),
            Value::Str(_) | Value::IntArray(_) | Value::FloatArray(_) | Value::StrArray(_) => None,
        }
    }

    /// The float, also from an integer, as the addon's float reader accepts both. A negative integer keeps its sign
    /// (the original converts it as unsigned, so -1 read as 4294967296.0).
    pub fn as_float(&self) -> Option<f32> {
        match self {
            Value::Float(v) => Some(*v),
            Value::Int(v) => Some(*v as f32),
            Value::Str(_) | Value::IntArray(_) | Value::FloatArray(_) | Value::StrArray(_) => None,
        }
    }

    /// The string bytes of a single string value.
    pub fn as_str(&self) -> Option<&[u8]> {
        match self {
            Value::Str(v) => Some(v),
            Value::Int(_)
            | Value::Float(_)
            | Value::IntArray(_)
            | Value::FloatArray(_)
            | Value::StrArray(_) => None,
        }
    }

    /// The floats of a float array, or of a single float. The original's array reader takes only that
    /// key type or the float scalar: a single integer is not an array (it reads as no key).
    pub fn as_float_array(&self) -> Option<Vec<f32>> {
        match self {
            Value::FloatArray(v) => Some(v.clone()),
            Value::Float(v) => Some(vec![*v]),
            Value::Int(_) | Value::Str(_) | Value::IntArray(_) | Value::StrArray(_) => None,
        }
    }

    /// The strings of a string array, or of a single string.
    pub fn as_str_array(&self) -> Option<Vec<&[u8]>> {
        match self {
            Value::StrArray(v) => Some(v.iter().map(Vec::as_slice).collect()),
            Value::Str(v) => Some(vec![v.as_slice()]),
            Value::Int(_) | Value::Float(_) | Value::IntArray(_) | Value::FloatArray(_) => None,
        }
    }
}

impl Section {
    /// The first key with this name, ignoring ASCII case.
    pub fn key(&self, name: &str) -> Option<&Key> {
        self.keys
            .iter()
            .find(|k| k.name.eq_ignore_ascii_case(name.as_bytes()))
    }

    /// The value of the first key with this name, ignoring ASCII case.
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.key(name).map(|k| &k.value)
    }
}

impl Registry {
    /// The first section with this name, ignoring ASCII case. A second section of the same name is never found,
    /// as in the addon.
    pub fn section(&self, name: &str) -> Option<&Section> {
        self.sections
            .iter()
            .find(|s| s.name.eq_ignore_ascii_case(name.as_bytes()))
    }

    /// The value of a key, ignoring ASCII case in both names.
    pub fn get(&self, section: &str, key: &str) -> Option<&Value> {
        self.section(section)?.get(key)
    }

    /// An integer key (a float key is truncated).
    pub fn get_int(&self, section: &str, key: &str) -> Option<i32> {
        self.get(section, key)?.as_int()
    }

    /// A float key (an integer key is converted).
    pub fn get_float(&self, section: &str, key: &str) -> Option<f32> {
        self.get(section, key)?.as_float()
    }

    /// A string key as raw bytes.
    pub fn get_str(&self, section: &str, key: &str) -> Option<&[u8]> {
        self.get(section, key)?.as_str()
    }

    /// A float array, or a single float as a one-element array.
    pub fn get_float_array(&self, section: &str, key: &str) -> Option<Vec<f32>> {
        self.get(section, key)?.as_float_array()
    }

    /// A string array, or a single string as a one-element array.
    pub fn get_str_array(&self, section: &str, key: &str) -> Option<Vec<&[u8]>> {
        self.get(section, key)?.as_str_array()
    }

    /// Reads a file.
    ///
    /// # Errors
    /// A [`ParseError`] naming the offset and what was expected there. Table slots, names, strings and arrays share
    /// a byte budget equal to the file size; this bounds their total size but does not detect every overlap.
    pub fn parse(data: &[u8]) -> Result<Registry, ParseError> {
        Parser {
            data,
            pos: 0,
            budget: data.len(),
        }
        .registry()
    }

    /// Writes the file: sections and keys in order, laid out back to back, with the hash tables built as
    /// `um-multitool inireg` builds them (an entry whose home slot is taken goes to the highest free slot and is
    /// linked at the end of the chain).
    ///
    /// # Errors
    /// A [`WriteError`] when a count, a length or an offset does not fit its field.
    pub fn to_bytes(&self) -> Result<Vec<u8>, WriteError> {
        let mut out = MAGIC.to_vec();
        out.extend_from_slice(
            &count_u16(self.sections.len(), "more than 65535 sections")?.to_le_bytes(),
        );
        let table = out.len();
        out.resize(table + 6 * self.sections.len(), 0);
        let mut offsets = Vec::with_capacity(self.sections.len());
        for section in &self.sections {
            offsets.push(offset_u32(out.len(), 0, "file larger than 4 GiB")?);
            write_section(&mut out, section)?;
        }
        let names: Vec<&[u8]> = self.sections.iter().map(|s| s.name.as_slice()).collect();
        fill_table(&mut out[table..], &names, &offsets);
        Ok(out)
    }
}

fn count_u16(n: usize, what: &'static str) -> Result<u16, WriteError> {
    u16::try_from(n).map_err(|_| WriteError { what })
}

fn offset_u32(at: usize, base: usize, what: &'static str) -> Result<u32, WriteError> {
    u32::try_from(at - base).map_err(|_| WriteError { what })
}

/// The home slot of a name: the sum of its lowercased bytes, taken as signed `char`s, modulo the table size.
fn home_slot(name: &[u8], slots: usize) -> usize {
    let sum = name.iter().fold(0u32, |acc, &b| {
        acc.wrapping_add(i32::from(b.to_ascii_lowercase() as i8) as u32)
    });
    sum as usize % slots
}

/// Fills a table of `names.len()` slots with `(next slot, offset)`, placing the entries in order.
fn fill_table(table: &mut [u8], names: &[&[u8]], offsets: &[u32]) {
    let n = names.len();
    // Per slot: the entry placed there and the next slot of its chain.
    let mut slots: Vec<Option<(usize, u16)>> = vec![None; n];
    let mut free = n;
    for (entry, name) in names.iter().enumerate() {
        let mut slot = home_slot(name, n);
        if slots[slot].is_some() {
            while let Some((_, next)) = slots[slot] {
                if next == CHAIN_END {
                    break;
                }
                slot = usize::from(next);
            }
            while slots[free - 1].is_some() {
                free -= 1;
            }
            free -= 1;
            // A table holds at most 65535 slots, so a slot number never reaches CHAIN_END.
            slots[slot] = slots[slot].map(|(e, _)| (e, free as u16));
            slot = free;
        }
        slots[slot] = Some((entry, CHAIN_END));
    }
    for (i, slot) in slots.iter().enumerate() {
        if let Some((entry, next)) = *slot {
            table[6 * i..6 * i + 2].copy_from_slice(&next.to_le_bytes());
            table[6 * i + 2..6 * i + 6].copy_from_slice(&offsets[entry].to_le_bytes());
        }
    }
}

fn write_section(out: &mut Vec<u8>, section: &Section) -> Result<(), WriteError> {
    let start = out.len();
    out.extend_from_slice(
        &count_u16(section.keys.len(), "more than 65535 keys in a section")?.to_le_bytes(),
    );
    write_name(out, &section.name, "section name longer than 65535 bytes")?;
    let table = out.len();
    out.resize(table + 6 * section.keys.len(), 0);
    let mut offsets = Vec::with_capacity(section.keys.len());
    for key in &section.keys {
        offsets.push(offset_u32(out.len(), start, "section larger than 4 GiB")?);
        write_key(out, key)?;
    }
    let names: Vec<&[u8]> = section.keys.iter().map(|k| k.name.as_slice()).collect();
    fill_table(&mut out[table..], &names, &offsets);
    Ok(())
}

fn write_name(out: &mut Vec<u8>, name: &[u8], what: &'static str) -> Result<(), WriteError> {
    out.extend_from_slice(&count_u16(name.len(), what)?.to_le_bytes());
    out.extend_from_slice(name);
    Ok(())
}

fn write_key(out: &mut Vec<u8>, key: &Key) -> Result<(), WriteError> {
    let (kind, array_len) = match &key.value {
        Value::Int(_) => (TYPE_INT, None),
        Value::Float(_) => (TYPE_FLOAT, None),
        Value::Str(_) => (TYPE_STR, None),
        Value::IntArray(v) => (TYPE_INT | ARRAY_FLAG, Some(v.len())),
        Value::FloatArray(v) => (TYPE_FLOAT | ARRAY_FLAG, Some(v.len())),
        Value::StrArray(v) => (TYPE_STR | ARRAY_FLAG, Some(v.len())),
    };
    out.push(kind);
    write_name(out, &key.name, "key name longer than 65535 bytes")?;
    if let Some(n) = array_len {
        out.extend_from_slice(&count_u16(n, "array of more than 65535 elements")?.to_le_bytes());
    }
    match &key.value {
        Value::Int(v) => out.extend_from_slice(&v.to_le_bytes()),
        Value::Float(v) => out.extend_from_slice(&v.to_le_bytes()),
        Value::Str(v) => write_name(out, v, "string longer than 65535 bytes")?,
        Value::IntArray(v) => v
            .iter()
            .for_each(|x| out.extend_from_slice(&x.to_le_bytes())),
        Value::FloatArray(v) => v
            .iter()
            .for_each(|x| out.extend_from_slice(&x.to_le_bytes())),
        Value::StrArray(v) => {
            for s in v {
                write_name(out, s, "string longer than 65535 bytes")?;
            }
        }
    }
    Ok(())
}

struct Parser<'a> {
    data: &'a [u8],
    pos: usize,
    /// Bytes the reader may still copy out of the file; see [`Registry::parse`].
    budget: usize,
}

impl Parser<'_> {
    fn err(&self, expected: &'static str) -> ParseError {
        ParseError {
            offset: self.pos,
            expected,
        }
    }

    fn take(&mut self, n: usize, expected: &'static str) -> Result<&[u8], ParseError> {
        let end = self
            .pos
            .checked_add(n)
            .filter(|&e| e <= self.data.len())
            .ok_or_else(|| self.err(expected))?;
        let bytes = &self.data[self.pos..end];
        self.pos = end;
        Ok(bytes)
    }

    fn u16(&mut self, expected: &'static str) -> Result<u16, ParseError> {
        let b = self.take(2, expected)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    fn u32(&mut self, expected: &'static str) -> Result<u32, ParseError> {
        let b = self.take(4, expected)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn charge(&mut self, n: usize) -> Result<(), ParseError> {
        self.budget = self
            .budget
            .checked_sub(n)
            .ok_or_else(|| self.err("data that no other section or key already uses"))?;
        Ok(())
    }

    fn seek(&mut self, offset: usize, expected: &'static str) -> Result<(), ParseError> {
        if offset > self.data.len() {
            self.pos = self.data.len();
            return Err(self.err(expected));
        }
        self.pos = offset;
        Ok(())
    }

    fn registry(&mut self) -> Result<Registry, ParseError> {
        if self.take(4, "magic FB 3E AB 45")? != MAGIC {
            self.pos = 0;
            return Err(self.err("magic FB 3E AB 45"));
        }
        let count = usize::from(self.u16("section count")?);
        let offsets = self.table(count, "next slot of the section table")?;
        let mut sections = Vec::with_capacity(offsets.len());
        for offset in offsets {
            self.seek(offset, "a section offset inside the file")?;
            sections.push(self.section(offset)?);
        }
        Ok(Registry { sections })
    }

    /// Reads a hash table of `count` slots and returns the entry offsets in file order. The chains only serve the
    /// game's lookups; the reader checks that each link stays inside the table.
    fn table(&mut self, count: usize, link: &'static str) -> Result<Vec<usize>, ParseError> {
        let mut offsets = Vec::with_capacity(count.min(self.data.len() / 6));
        for _ in 0..count {
            self.charge(6)?;
            let next = self.u16(link)?;
            if next != CHAIN_END && usize::from(next) >= count {
                self.pos -= 2;
                return Err(self.err("a chain link to a slot of the table, or FFFF"));
            }
            offsets.push(self.u32("entry offset in the table")? as usize);
        }
        offsets.sort_unstable();
        Ok(offsets)
    }

    fn section(&mut self, start: usize) -> Result<Section, ParseError> {
        let key_count = usize::from(self.u16("key count of the section")?);
        let name = self.name("section name length", "section name")?;
        let offsets = self.table(key_count, "next slot of the key table")?;
        let mut keys = Vec::with_capacity(offsets.len());
        for rel in offsets {
            self.seek(start.saturating_add(rel), "a key offset inside the file")?;
            keys.push(self.key()?);
        }
        Ok(Section { name, keys })
    }

    fn name(&mut self, len_what: &'static str, what: &'static str) -> Result<Vec<u8>, ParseError> {
        let len = usize::from(self.u16(len_what)?);
        self.charge(len)?;
        Ok(self.take(len, what)?.to_vec())
    }

    fn key(&mut self) -> Result<Key, ParseError> {
        let kind = self.take(1, "key type byte")?[0];
        let base = kind & !ARRAY_FLAG;
        if base > TYPE_STR {
            self.pos -= 1;
            return Err(
                self.err("key type 0 (int), 1 (float) or 2 (string), plus 0x80 for an array")
            );
        }
        let name = self.name("key name length", "key name")?;
        let value = if kind & ARRAY_FLAG == 0 {
            match base {
                TYPE_INT => Value::Int(self.int()?),
                TYPE_FLOAT => Value::Float(self.float()?),
                _ => Value::Str(self.name("string length", "string bytes")?),
            }
        } else {
            let n = usize::from(self.u16("array element count")?);
            match base {
                TYPE_INT => {
                    self.charge(4 * n)?;
                    Value::IntArray((0..n).map(|_| self.int()).collect::<Result<_, _>>()?)
                }
                TYPE_FLOAT => {
                    self.charge(4 * n)?;
                    Value::FloatArray((0..n).map(|_| self.float()).collect::<Result<_, _>>()?)
                }
                _ => {
                    self.charge(2 * n)?;
                    Value::StrArray(
                        (0..n)
                            .map(|_| self.name("string length", "string bytes"))
                            .collect::<Result<_, _>>()?,
                    )
                }
            }
        };
        Ok(Key { name, value })
    }

    fn int(&mut self) -> Result<i32, ParseError> {
        Ok(self.u32("4-byte integer")? as i32)
    }

    fn float(&mut self) -> Result<f32, ParseError> {
        Ok(f32::from_bits(self.u32("4-byte float")?))
    }
}
