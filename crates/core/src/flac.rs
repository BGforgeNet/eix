//! FLAC music: a FLAC stream decoded into the 16-bit interleaved PCM the game's music player takes from its decoder
//! (`docs/addon.md`, FLAC music).

use std::io::Read;

/// The first four bytes of a FLAC stream.
pub const MAGIC: [u8; 4] = *b"fLaC";

/// Why a stream does not decode.
pub type Error = claxon::Error;

/// The decoded audio's format; samples are always 16 bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Format {
    /// Samples per second.
    pub sample_rate: u32,
    /// 1 or 2: the game's player takes mono and stereo.
    pub channels: u32,
}

/// A FLAC stream read from `R`, decoded a block at a time.
pub struct Decoder<R: Read> {
    reader: claxon::FlacReader<R>,
    format: Format,
    bits: u32,
    /// The last block's samples, reused for the next block.
    samples: Vec<i32>,
    /// PCM of the last block not handed out yet, from `at`.
    pcm: Vec<u8>,
    at: usize,
    ended: bool,
}

impl<R: Read> core::fmt::Debug for Decoder<R> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Decoder")
            .field("format", &self.format)
            .field("bits", &self.bits)
            .field("ended", &self.ended)
            .finish_non_exhaustive()
    }
}

impl<R: Read> Decoder<R> {
    /// Reads the stream's header; a stream of more than two channels, or of more than 32 bits a sample, is refused.
    pub fn new(source: R) -> Result<Self, Error> {
        let reader = claxon::FlacReader::new(source)?;
        let info = reader.streaminfo();
        if !matches!(info.channels, 1 | 2) {
            return Err(Error::Unsupported("more than two channels"));
        }
        if info.bits_per_sample > 32 {
            return Err(Error::Unsupported("more than 32 bits a sample"));
        }
        Ok(Self {
            reader,
            format: Format {
                sample_rate: info.sample_rate,
                channels: info.channels,
            },
            bits: info.bits_per_sample,
            samples: Vec::new(),
            pcm: Vec::new(),
            at: 0,
            ended: false,
        })
    }

    /// The decoded audio's format.
    pub fn format(&self) -> Format {
        self.format
    }

    /// Fills `out` with PCM and returns the bytes written: all of `out` until the stream ends.
    pub fn read(&mut self, out: &mut [u8]) -> Result<usize, Error> {
        let mut written = 0;
        while written < out.len() {
            if self.at == self.pcm.len() && !self.next_block()? {
                break;
            }
            let (from, to) = (&self.pcm[self.at..], &mut out[written..]);
            let n = from.len().min(to.len());
            to[..n].copy_from_slice(&from[..n]);
            self.at += n;
            written += n;
        }
        Ok(written)
    }

    /// Decodes the next block into `pcm`; false at the end of the stream.
    fn next_block(&mut self) -> Result<bool, Error> {
        if self.ended {
            return Ok(false);
        }
        let buffer = core::mem::take(&mut self.samples);
        let Some(block) = self.reader.blocks().read_next_or_eof(buffer)? else {
            self.ended = true;
            return Ok(false);
        };
        self.pcm.clear();
        self.at = 0;
        if block.channels() == 2 {
            for (left, right) in block.stereo_samples() {
                for sample in [left, right] {
                    let sample = to_16_bits(sample, self.bits);
                    self.pcm.extend_from_slice(&sample.to_le_bytes());
                }
            }
        } else {
            for &sample in block.channel(0) {
                let sample = to_16_bits(sample, self.bits);
                self.pcm.extend_from_slice(&sample.to_le_bytes());
            }
        }
        self.samples = block.into_buffer();
        Ok(true)
    }
}

/// The FLAC files to try, in order, for the stream file `path` the game opens: the mod's `Stream\` folder
/// (`mod_stream`, ending in `\`, empty without a mod) and then `path`'s own folder, with the name's `.mp3` replaced by
/// `.flac`. None for a path that does not end in `.mp3`. The game's folder choice keeps its order: a mod's `.mp3`
/// is the path when the mod has one, so a game folder's `.flac` never wins over it.
pub fn candidates(path: &[u8], mod_stream: &[u8]) -> Vec<Vec<u8>> {
    let Some(stem) = path
        .len()
        .checked_sub(4)
        .filter(|&at| path[at..].eq_ignore_ascii_case(b".mp3"))
        .map(|at| &path[..at])
    else {
        return Vec::new();
    };
    let name = stem
        .iter()
        .rposition(|&c| c == b'\\' || c == b'/')
        .map_or(stem, |at| &stem[at + 1..]);
    let mut found = Vec::new();
    if !mod_stream.is_empty() {
        found.push([mod_stream, name, b".flac"].concat());
    }
    let own = [stem, b".flac"].concat();
    if !found.iter().any(|f| f.eq_ignore_ascii_case(&own)) {
        found.push(own);
    }
    found
}

/// A sample of `bits` bits as a 16-bit one: wider samples lose their low bits, narrower ones are scaled up.
fn to_16_bits(sample: i32, bits: u32) -> i16 {
    let scaled = if bits > 16 {
        sample >> (bits - 16)
    } else {
        sample << (16 - bits)
    };
    // In range after the shift for a sample that fits its `bits`; a stream out of range is clamped.
    let clamped = scaled.clamp(i16::MIN.into(), i16::MAX.into());
    i16::try_from(clamped).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    const STEREO_16: &[u8] = include_bytes!("../tests/data/flac/stereo16.flac");
    const MONO_24: &[u8] = include_bytes!("../tests/data/flac/mono24.flac");

    /// The fixtures' samples: a sawtooth per channel, as the WAV sources the reference encoder took were written.
    fn source_sample(i: i64, channel: i64, bits: u32) -> i64 {
        let span = 1i64 << bits;
        (i * (977 + 131 * channel) + channel * 12345) % span - span / 2
    }

    fn decode_all(data: &[u8], chunk: usize) -> (Format, Vec<u8>) {
        let mut decoder = Decoder::new(data).unwrap();
        let mut pcm = Vec::new();
        let mut buffer = vec![0; chunk];
        loop {
            let n = decoder.read(&mut buffer).unwrap();
            pcm.extend_from_slice(&buffer[..n]);
            if n < chunk {
                break;
            }
        }
        (decoder.format(), pcm)
    }

    fn expected(frames: i64, channels: i64, bits: u32) -> Vec<u8> {
        let mut pcm = Vec::new();
        for i in 0..frames {
            for channel in 0..channels {
                let sample = source_sample(i, channel, bits);
                let sample = i16::try_from(sample >> (bits - 16)).unwrap();
                pcm.extend_from_slice(&sample.to_le_bytes());
            }
        }
        pcm
    }

    #[test]
    fn a_16_bit_stereo_stream_decodes_to_its_samples() {
        let (format, pcm) = decode_all(STEREO_16, 4096);
        assert_eq!(
            format,
            Format {
                sample_rate: 44100,
                channels: 2
            }
        );
        assert_eq!(pcm, expected(5000, 2, 16));
    }

    #[test]
    fn a_24_bit_mono_stream_keeps_its_high_16_bits() {
        let (format, pcm) = decode_all(MONO_24, 4096);
        assert_eq!(format.channels, 1);
        assert_eq!(pcm, expected(3000, 1, 24));
    }

    #[test]
    fn any_read_size_gives_the_same_pcm() {
        let whole = decode_all(STEREO_16, 1 << 20).1;
        for chunk in [1, 3, 4096, 4097] {
            assert_eq!(decode_all(STEREO_16, chunk).1, whole, "chunk {chunk}");
        }
    }

    #[test]
    fn reads_after_the_end_give_nothing() {
        let mut decoder = Decoder::new(STEREO_16).unwrap();
        let mut all = vec![0; 30_000];
        assert_eq!(decoder.read(&mut all).unwrap(), 20_000);
        assert_eq!(decoder.read(&mut all).unwrap(), 0);
    }

    #[test]
    fn a_stream_that_is_no_flac_is_refused() {
        assert!(Decoder::new(&b"ID3\x03not flac at all"[..]).is_err());
        assert_eq!(&STEREO_16[..4], &MAGIC);
    }

    #[test]
    fn a_track_tries_the_mods_flac_then_its_own_folders() {
        let to = |list: Vec<Vec<u8>>| {
            list.into_iter()
                .map(|p| String::from_utf8(p).unwrap())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            to(candidates(
                b"C:\\Game\\Stream\\wild.mp3",
                b"C:\\Mods\\m\\Stream\\"
            )),
            [
                "C:\\Mods\\m\\Stream\\wild.flac",
                "C:\\Game\\Stream\\wild.flac"
            ]
        );
        // The game chose the mod's .mp3: its folder's .flac is the mod's, tried once.
        assert_eq!(
            to(candidates(
                b"C:\\Mods\\m\\Stream\\wild.MP3",
                b"C:\\Mods\\m\\Stream\\"
            )),
            ["C:\\Mods\\m\\Stream\\wild.flac"]
        );
        assert_eq!(
            to(candidates(b"Stream\\main.mp3", b"")),
            ["Stream\\main.flac"]
        );
        assert!(candidates(b"Stream\\main.wav", b"").is_empty());
        assert!(candidates(b"mp3", b"").is_empty());
    }

    #[test]
    fn narrow_and_wide_samples_scale_to_16_bits() {
        assert_eq!(to_16_bits(-128, 8), -32768);
        assert_eq!(to_16_bits(127, 8), 32512);
        assert_eq!(to_16_bits(0x7F_FFFF, 24), 0x7FFF);
        assert_eq!(to_16_bits(-0x80_0000, 24), -0x8000);
        assert_eq!(to_16_bits(1234, 16), 1234);
    }
}
