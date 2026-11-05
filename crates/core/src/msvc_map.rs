//! The element order of MSVC 2010's `std::unordered_map<std::string, float>`, which PersistentGlobalVars keeps the
//! `gvar.` variables in and sets them back from: that order decides their order in the game's variable table.
//!
//! The container keeps every element in one list, with each bucket's elements as one run of it. A new key goes in
//! front of its bucket's run, or to the end of the list when the bucket is empty. It starts with 8 buckets; once there
//! are more elements than buckets, it multiplies the buckets by 8 and places every element again in list order.

/// Buckets of a new map.
const MIN_BUCKETS: usize = 8;

/// One element and its key's hash.
#[derive(Debug, Clone)]
struct Element {
    key: Vec<u8>,
    hash: u32,
    value: f32,
}

/// The map, as far as its element order goes.
#[derive(Debug, Clone)]
pub struct StringFloatMap {
    /// In list order.
    elements: Vec<Element>,
    /// A power of two.
    buckets: usize,
}

impl Default for StringFloatMap {
    fn default() -> Self {
        Self {
            elements: Vec::new(),
            buckets: MIN_BUCKETS,
        }
    }
}

/// `std::hash<std::string>` of MSVC 2010: FNV-1 over every `len / 10 + 1`-th byte, each byte sign-extended (`char` is
/// signed there).
fn hash(key: &[u8]) -> u32 {
    let step = key.len() / 10 + 1;
    let mut h: u32 = 2_166_136_261;
    for &byte in key.iter().step_by(step) {
        h = h.wrapping_mul(16_777_619) ^ i32::from(byte.cast_signed()).cast_unsigned();
    }
    h
}

/// Places `element` in `list` for a table of `buckets` buckets.
fn place(list: &mut Vec<Element>, buckets: usize, element: Element) {
    let mask = buckets as u32 - 1;
    let bucket = element.hash & mask;
    match list.iter().position(|e| e.hash & mask == bucket) {
        Some(run) => list.insert(run, element),
        None => list.push(element),
    }
}

impl StringFloatMap {
    /// `operator[]`: the value for `key`, added as 0 when the map lacks it.
    pub fn entry(&mut self, key: &[u8]) -> &mut f32 {
        let index = match self.elements.iter().position(|e| e.key == key) {
            Some(i) => i,
            None => self.add(key),
        };
        &mut self.elements[index].value
    }

    /// Adds `key` with 0 and returns its index.
    fn add(&mut self, key: &[u8]) -> usize {
        let element = Element {
            key: key.to_vec(),
            hash: hash(key),
            value: 0.0,
        };
        place(&mut self.elements, self.buckets, element);
        if self.elements.len() > self.buckets {
            self.buckets *= 8;
            let old = core::mem::take(&mut self.elements);
            for e in old {
                place(&mut self.elements, self.buckets, e);
            }
        }
        self.elements
            .iter()
            .position(|e| e.key == key)
            .unwrap_or_else(|| unreachable!("{key:?} was just placed"))
    }

    /// The elements in list order.
    pub fn elements(&self) -> Vec<(&[u8], f32)> {
        self.elements
            .iter()
            .map(|e| (&e.key[..], e.value))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASIS: u32 = 2_166_136_261;
    const PRIME: u32 = 16_777_619;

    fn keys(m: &StringFloatMap) -> Vec<String> {
        m.elements()
            .iter()
            .map(|(k, _)| String::from_utf8_lossy(k).into_owned())
            .collect()
    }

    #[test]
    fn the_hash_steps_through_long_keys() {
        assert_eq!(hash(b""), BASIS);
        assert_eq!(hash(b"a"), BASIS.wrapping_mul(PRIME) ^ 0x61);
        // Eleven bytes: every second one.
        let every_second = b"acegik"
            .iter()
            .fold(BASIS, |h, &c| h.wrapping_mul(PRIME) ^ u32::from(c));
        assert_eq!(hash(b"abcdefghijk"), every_second);
        // A byte above 0x7F counts as negative.
        assert_eq!(hash(&[0xE9]), BASIS.wrapping_mul(PRIME) ^ 0xFFFF_FFE9);
    }

    #[test]
    fn a_key_is_kept_once_with_its_last_value() {
        let mut m = StringFloatMap::default();
        *m.entry(b"gvar.a") = 1.0;
        *m.entry(b"gvar.b") = 2.0;
        *m.entry(b"gvar.a") = 3.0;
        let e = m.elements();
        assert_eq!(e.len(), 2);
        assert!(e.contains(&(&b"gvar.a"[..], 3.0)));
        assert!(e.contains(&(&b"gvar.b"[..], 2.0)));
    }

    #[test]
    fn a_new_key_goes_in_front_of_its_bucket_or_to_the_end() {
        let mut m = StringFloatMap::default();
        for k in ["gvar.a", "gvar.b", "gvar.i", "gvar.c"] {
            *m.entry(k.as_bytes()) = 1.0;
        }
        // "gvar.a" and "gvar.i" share a bucket of 8 (their last bytes differ by 8); "gvar.b" and "gvar.c" have their
        // own, so each went to the end.
        assert_eq!(hash(b"gvar.a") & 7, hash(b"gvar.i") & 7);
        assert_eq!(keys(&m), ["gvar.i", "gvar.a", "gvar.b", "gvar.c"]);
    }

    #[test]
    fn the_ninth_key_grows_the_table_and_reorders() {
        let last_bytes =
            |m: &StringFloatMap| -> Vec<u8> { m.elements().iter().map(|(k, _)| k[5]).collect() };
        let mut m = StringFloatMap::default();
        // 0x41, 0x49 and 0x81 share a bucket of 8; of 64, only 0x41 and 0x81 (0x81 is sign-extended, its low bits
        // stay). 0x42 to 0x47 each have a bucket of their own.
        for last in [0x41, 0x49, 0x81, 0x42, 0x43, 0x44, 0x45, 0x46] {
            *m.entry(&[b'g', b'v', b'a', b'r', b'.', last]) = 1.0;
        }
        assert_eq!(
            last_bytes(&m),
            [0x81, 0x49, 0x41, 0x42, 0x43, 0x44, 0x45, 0x46]
        );
        *m.entry(b"gvar.G") = 1.0;
        assert_eq!(m.buckets, 64);
        assert_eq!(
            last_bytes(&m),
            [0x41, 0x81, 0x49, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47]
        );
    }

    #[test]
    fn each_bucket_stays_one_run() {
        let mut m = StringFloatMap::default();
        let names: Vec<Vec<u8>> = (0..40).map(|i| format!("gvar.v{i}").into_bytes()).collect();
        for (i, k) in names.iter().enumerate() {
            *m.entry(k) = i as f32;
        }
        let e = m.elements();
        assert_eq!(e.len(), 40);
        for (i, k) in names.iter().enumerate() {
            assert!(e.contains(&(&k[..], i as f32)));
        }
        let mask = m.buckets as u32 - 1;
        let buckets: Vec<u32> = e.iter().map(|(k, _)| hash(k) & mask).collect();
        for (i, b) in buckets.iter().enumerate() {
            let last = buckets.iter().rposition(|x| x == b).unwrap_or(i);
            assert!(buckets[i..=last].iter().all(|x| x == b), "bucket {b} split");
        }
    }
}
