//! FLAC music: a FLAC stream decoded into the 16-bit interleaved PCM the game's music player takes from its decoder
//! (`docs/engine.md`, FLAC music).

use std::io::Read;

/// The first four bytes of a FLAC stream.
pub const MAGIC: [u8; 4] = *b"fLaC";

/// The Vorbis comment that sets a track's gain, in decibels, as `-10.2` or `-10.2 dB`. It wins over [`REPLAYGAIN_TAG`],
/// for a track whose level the game's music sets and a reference loudness does not.
pub const GAIN_TAG: &str = "EIX_GAIN";

/// The standard ReplayGain track gain, read when a track has no [`GAIN_TAG`].
pub const REPLAYGAIN_TAG: &str = "REPLAYGAIN_TRACK_GAIN";

/// The gain tag's range in decibels; a value outside it is not a gain.
const GAIN_RANGE_DB: core::ops::RangeInclusive<f64> = -100.0..=100.0;

/// A gain of less than this many decibels is no gain: a 0 dB track stays bit for bit, and 0.005 dB is 0.06 %.
const UNITY_DB: f64 = 0.005;

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

/// A track's gain tag read: the decibels to apply, or the text that is not a gain.
#[derive(Debug, Clone, PartialEq)]
enum Gain {
    /// No tag, or a tag of about 0 dB: samples pass bit for bit when they are 16 bits.
    Unity,
    Db(f64),
    /// A tag whose text is no gain: its name and its text.
    Invalid(&'static str, String),
}

/// The decibels in a gain tag's value: a number, with or without a trailing `dB`, within [`GAIN_RANGE_DB`].
fn parse_gain_db(value: &str) -> Option<f64> {
    let value = value.trim();
    let number = match value.len().checked_sub(2) {
        Some(at) if value.is_char_boundary(at) && value[at..].eq_ignore_ascii_case("db") => {
            &value[..at]
        }
        _ => value,
    };
    number
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|db| GAIN_RANGE_DB.contains(db))
}

/// Noise for the last rounding: two uniform draws from an xorshift generator make a triangular one, which keeps the
/// rounding error independent of the signal. Fixed seed, so a track decodes the same every time.
#[derive(Debug, Clone)]
struct Dither(u32);

impl Dither {
    fn new() -> Self {
        Self(0x2545_F491)
    }

    /// A uniform value in [0, 1).
    fn unit(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        f64::from(self.0) / 4_294_967_296.0
    }

    /// A triangular value in (-1, 1), in units of the last bit.
    fn triangular(&mut self) -> f64 {
        self.unit() + self.unit() - 1.0
    }
}

/// A FLAC stream read from `R`, decoded a block at a time.
pub struct Decoder<R: Read> {
    reader: claxon::FlacReader<R>,
    format: Format,
    bits: u32,
    gain: Gain,
    /// The linear factor of `gain`, 1 for no gain.
    factor: f64,
    dither: Dither,
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
            .field("gain", &self.gain)
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
        let tag = [GAIN_TAG, REPLAYGAIN_TAG]
            .into_iter()
            .find_map(|name| reader.get_tag(name).next().map(|text| (name, text)));
        let gain = match tag {
            None => Gain::Unity,
            Some((name, text)) => match parse_gain_db(text) {
                Some(db) if db.abs() < UNITY_DB => Gain::Unity,
                Some(db) => Gain::Db(db),
                None => Gain::Invalid(name, text.to_owned()),
            },
        };
        let factor = match gain {
            Gain::Db(db) => 10f64.powf(db / 20.0),
            Gain::Unity | Gain::Invalid(..) => 1.0,
        };
        Ok(Self {
            format: Format {
                sample_rate: info.sample_rate,
                channels: info.channels,
            },
            bits: info.bits_per_sample,
            reader,
            gain,
            factor,
            dither: Dither::new(),
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

    /// The gain in decibels the stream's tag sets; `None` for a stream that plays at its own level.
    pub fn gain_db(&self) -> Option<f64> {
        match self.gain {
            Gain::Db(db) => Some(db),
            Gain::Unity | Gain::Invalid(..) => None,
        }
    }

    /// The name and text of a gain tag that is not a gain; the stream plays at its own level.
    pub fn invalid_gain(&self) -> Option<(&'static str, &str)> {
        match &self.gain {
            Gain::Invalid(name, text) => Some((name, text)),
            Gain::Unity | Gain::Db(_) => None,
        }
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
        let gain = matches!(self.gain, Gain::Db(_)).then_some(self.factor);
        if block.channels() == 2 {
            for (left, right) in block.stereo_samples() {
                for sample in [left, right] {
                    let sample = to_16_bits(sample, self.bits, gain, &mut self.dither);
                    self.pcm.extend_from_slice(&sample.to_le_bytes());
                }
            }
        } else {
            for &sample in block.channel(0) {
                let sample = to_16_bits(sample, self.bits, gain, &mut self.dither);
                self.pcm.extend_from_slice(&sample.to_le_bytes());
            }
        }
        self.samples = block.into_buffer();
        Ok(true)
    }
}

/// How long the music at a loop's cut fades out over the restart, in milliseconds: about what the MP3 decoder's own
/// overlap carries past a seek (an MP3 granule is 576 frames, 13 ms at 44.1 kHz).
pub const SEAM_FADE_MS: u32 = 10;

/// The music a decoder would have played on from a loop's cut, faded out, to lay over the restart's first frames. The
/// game cuts a looping track where its block counter lands, often mid-phrase, and the restart begins in silence: a cut
/// straight to it clicks, where the MP3 decoder's leftover overlap carries the music down.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SeamFade {
    /// Interleaved 16-bit samples, the fade applied; `at` is the next one to lay over.
    samples: Vec<i16>,
    at: usize,
}

impl SeamFade {
    /// The next [`SEAM_FADE_MS`] of `decoder`, faded from full level to silence by a raised cosine; shorter or empty
    /// at the stream's end.
    pub fn take<R: Read>(decoder: &mut Decoder<R>) -> Result<Self, Error> {
        let format = decoder.format();
        let frames = usize::try_from(format.sample_rate * SEAM_FADE_MS / 1000).unwrap_or(0);
        let channels = usize::try_from(format.channels).unwrap_or(1);
        let mut pcm = vec![0; frames * channels * 2];
        let n = decoder.read(&mut pcm)?;
        let samples = pcm[..n]
            .as_chunks::<2>()
            .0
            .iter()
            .enumerate()
            .map(|(i, bytes)| {
                let frame = (i / channels) as f64;
                let level =
                    0.5 * (1.0 + (core::f64::consts::PI * (frame + 0.5) / frames as f64).cos());
                let faded = (f64::from(i16::from_le_bytes(*bytes)) * level).round();
                // Within the 16-bit range: a 16-bit sample times a level of at most 1.
                faded as i16
            })
            .collect();
        Ok(Self { samples, at: 0 })
    }

    /// Adds the fade's next samples onto the 16-bit interleaved `pcm`, clamped to the 16-bit range.
    pub fn mix(&mut self, pcm: &mut [u8]) {
        for bytes in pcm.as_chunks_mut::<2>().0 {
            let Some(&fade) = self.samples.get(self.at) else {
                return;
            };
            *bytes = i16::from_le_bytes(*bytes)
                .saturating_add(fade)
                .to_le_bytes();
            self.at += 1;
        }
    }

    /// True once every sample has been laid over.
    pub fn is_done(&self) -> bool {
        self.at >= self.samples.len()
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

/// A sample of `bits` bits as a 16-bit one, times `gain` when there is one. A narrower sample without a gain is scaled
/// up exactly, and a 16-bit one passes as it is. Any other result needs rounding, which takes dither, so that the
/// error stays noise and does not follow the music.
fn to_16_bits(sample: i32, bits: u32, gain: Option<f64>, dither: &mut Dither) -> i16 {
    if gain.is_none() && bits <= 16 {
        let scaled = sample << (16 - bits);
        // In range for a sample that fits its `bits`; a stream out of range is clamped.
        let clamped = scaled.clamp(i16::MIN.into(), i16::MAX.into());
        return i16::try_from(clamped).unwrap_or_default();
    }
    let exact =
        f64::from(sample) * 2f64.powi(16 - i32::try_from(bits).unwrap_or(16)) * gain.unwrap_or(1.0);
    let rounded = (exact + dither.triangular()).round();
    // Clamped to the 16-bit range first, so the conversion is exact.
    i16::try_from(rounded.clamp(i16::MIN.into(), i16::MAX.into()) as i32).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    const STEREO_16: &[u8] = include_bytes!("../tests/data/flac/stereo16.flac");
    const MONO_24: &[u8] = include_bytes!("../tests/data/flac/mono24.flac");
    /// `stereo16.flac` with `EIX_GAIN=-6.0206 dB` and with `EIX_GAIN=loud` added by the reference `metaflac`.
    const STEREO_16_GAIN: &[u8] = include_bytes!("../tests/data/flac/stereo16-gain.flac");
    const STEREO_16_BAD_GAIN: &[u8] = include_bytes!("../tests/data/flac/stereo16-badgain.flac");

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

    /// The 16-bit samples of interleaved PCM.
    fn samples(pcm: &[u8]) -> Vec<i16> {
        pcm.as_chunks::<2>()
            .0
            .iter()
            .map(|b| i16::from_le_bytes(*b))
            .collect()
    }

    /// The fixtures' samples times `factor`, in 16-bit units, one per decoded sample.
    fn exact(frames: i64, channels: i64, bits: u32, factor: f64) -> Vec<f64> {
        (0..frames)
            .flat_map(|i| (0..channels).map(move |c| (i, c)))
            .map(|(i, c)| {
                source_sample(i, c, bits) as f64 / f64::from(1u32 << (bits - 16)) * factor
            })
            .collect()
    }

    /// Asserts each decoded sample is within a rounding and a dither of `exact` (1.5 of the last bit), and that the
    /// error has no bias.
    fn assert_rounded(pcm: &[u8], exact: &[f64]) {
        let got = samples(pcm);
        assert_eq!(got.len(), exact.len());
        let mut total = 0.0;
        for (g, e) in got.iter().zip(exact) {
            let error = f64::from(*g) - e;
            assert!(error.abs() <= 1.5, "{g} for {e}");
            total += error;
        }
        assert!((total / exact.len() as f64).abs() < 0.05, "biased");
    }

    #[test]
    fn a_24_bit_mono_stream_is_dithered_to_16_bits() {
        let (format, pcm) = decode_all(MONO_24, 4096);
        assert_eq!(format.channels, 1);
        assert_rounded(&pcm, &exact(3000, 1, 24, 1.0));
    }

    #[test]
    fn a_gain_tag_scales_the_stream() {
        let decoder = Decoder::new(STEREO_16_GAIN).unwrap();
        assert_eq!(decoder.gain_db(), Some(-6.0206));
        assert_eq!(decoder.invalid_gain(), None);
        // 6.0206 dB is half the amplitude.
        assert_rounded(
            &decode_all(STEREO_16_GAIN, 4096).1,
            &exact(5000, 2, 16, 0.5),
        );
    }

    #[test]
    fn a_stream_without_a_gain_tag_has_no_gain() {
        assert_eq!(Decoder::new(STEREO_16).unwrap().gain_db(), None);
    }

    #[test]
    fn a_gain_tag_that_is_no_number_leaves_the_stream_alone() {
        let decoder = Decoder::new(STEREO_16_BAD_GAIN).unwrap();
        assert_eq!(decoder.gain_db(), None);
        assert_eq!(decoder.invalid_gain(), Some((GAIN_TAG, "loud")));
        assert_eq!(
            decode_all(STEREO_16_BAD_GAIN, 4096).1,
            expected(5000, 2, 16)
        );
    }

    /// `base` with its Vorbis comment block replaced by one holding `tags`, written by hand so a test needs no
    /// fixture per combination.
    fn with_tags(base: &[u8], tags: &[(&str, &str)]) -> Vec<u8> {
        const VORBIS_COMMENT: u8 = 4;
        let mut out = base[..4].to_vec();
        let mut at = 4;
        loop {
            let header = base[at];
            let len = usize::from(base[at + 1]) << 16
                | usize::from(base[at + 2]) << 8
                | usize::from(base[at + 3]);
            if header & 0x7F != VORBIS_COMMENT {
                // Not the last block any more: the comment block follows.
                out.push(header & 0x7F);
                out.extend_from_slice(&base[at + 1..at + 4 + len]);
            }
            at += 4 + len;
            if header & 0x80 != 0 {
                break;
            }
        }
        let mut body = Vec::new();
        body.extend_from_slice(&0u32.to_le_bytes()); // empty vendor string
        body.extend_from_slice(&u32::try_from(tags.len()).unwrap().to_le_bytes());
        for (name, value) in tags {
            let comment = format!("{name}={value}");
            body.extend_from_slice(&u32::try_from(comment.len()).unwrap().to_le_bytes());
            body.extend_from_slice(comment.as_bytes());
        }
        let len = u32::try_from(body.len()).unwrap().to_be_bytes();
        out.extend_from_slice(&[0x80 | VORBIS_COMMENT, len[1], len[2], len[3]]);
        out.extend_from_slice(&body);
        out.extend_from_slice(&base[at..]);
        out
    }

    #[test]
    fn the_replaygain_track_gain_sets_the_gain() {
        for tag in ["REPLAYGAIN_TRACK_GAIN", "replaygain_track_gain"] {
            let data = with_tags(
                STEREO_16,
                &[(tag, "-6.02 dB"), ("REPLAYGAIN_TRACK_PEAK", "0.98")],
            );
            assert_eq!(
                Decoder::new(&data[..]).unwrap().gain_db(),
                Some(-6.02),
                "{tag}"
            );
        }
    }

    #[test]
    fn the_eix_gain_wins_over_the_replaygain_track_gain() {
        let data = with_tags(
            STEREO_16,
            &[("REPLAYGAIN_TRACK_GAIN", "-6.02 dB"), (GAIN_TAG, "-3 dB")],
        );
        assert_eq!(Decoder::new(&data[..]).unwrap().gain_db(), Some(-3.0));
    }

    #[test]
    fn an_invalid_replaygain_tag_names_itself() {
        let data = with_tags(STEREO_16, &[("REPLAYGAIN_TRACK_GAIN", "quiet")]);
        let decoder = Decoder::new(&data[..]).unwrap();
        assert_eq!(decoder.gain_db(), None);
        assert_eq!(decoder.invalid_gain(), Some((REPLAYGAIN_TAG, "quiet")));
    }

    #[test]
    fn the_album_gain_is_no_gain() {
        let data = with_tags(STEREO_16, &[("REPLAYGAIN_ALBUM_GAIN", "-6.02 dB")]);
        assert_eq!(Decoder::new(&data[..]).unwrap().gain_db(), None);
    }

    #[test]
    fn gain_text_is_decibels_with_or_without_the_unit() {
        for (text, db) in [
            ("-10.2", -10.2),
            ("-10.2 dB", -10.2),
            (" -6DB ", -6.0),
            ("+3.5 db", 3.5),
            ("0", 0.0),
            ("-100", -100.0),
        ] {
            assert_eq!(parse_gain_db(text), Some(db), "{text:?}");
        }
        for text in [
            "", "dB", "loud", "NaN", "inf", "-100.5", "101", "6 dB dB", "\u{e9}",
        ] {
            assert_eq!(parse_gain_db(text), None, "{text:?}");
        }
    }

    #[test]
    fn any_read_size_gives_the_same_pcm() {
        for data in [STEREO_16, STEREO_16_GAIN] {
            let whole = decode_all(data, 1 << 20).1;
            for chunk in [1, 3, 4096, 4097] {
                assert_eq!(decode_all(data, chunk).1, whole, "chunk {chunk}");
            }
        }
    }

    #[test]
    fn a_stream_decodes_the_same_every_time() {
        assert_eq!(
            decode_all(STEREO_16_GAIN, 4096),
            decode_all(STEREO_16_GAIN, 4096)
        );
    }

    #[test]
    fn reads_after_the_end_give_nothing() {
        let mut decoder = Decoder::new(STEREO_16).unwrap();
        let mut all = vec![0; 30_000];
        assert_eq!(decoder.read(&mut all).unwrap(), 20_000);
        assert_eq!(decoder.read(&mut all).unwrap(), 0);
    }

    #[test]
    fn a_seam_fade_is_the_music_on_from_the_cut_faded_to_silence() {
        let cut = 1000 * 4;
        let mut decoder = Decoder::new(STEREO_16).unwrap();
        let mut played = vec![0; cut];
        assert_eq!(decoder.read(&mut played).unwrap(), cut);
        let mut fade = SeamFade::take(&mut decoder).unwrap();
        // Laid over silence, the fade is what the stream would have played on, times the fade's level.
        let mut silence = vec![0; 441 * 4 + 8];
        fade.mix(&mut silence);
        assert!(fade.is_done());
        let got = samples(&silence);
        let on = samples(&expected(5000, 2, 16)[cut..cut + 441 * 4]);
        for frame in 0..441 {
            let level = 0.5 * (1.0 + (core::f64::consts::PI * (frame as f64 + 0.5) / 441.0).cos());
            for c in 0..2 {
                let want = (f64::from(on[frame * 2 + c]) * level).round();
                assert_eq!(f64::from(got[frame * 2 + c]), want, "frame {frame}");
            }
        }
        // Full level at the cut, silent at the end, nothing past it.
        assert_eq!(got[..2], on[..2]);
        assert!(got[880..882].iter().all(|s| s.abs() <= 1));
        assert!(got[882..].iter().all(|&s| s == 0));
    }

    #[test]
    fn a_seam_fade_adds_onto_the_restart_and_clamps() {
        let mut fade = SeamFade {
            samples: vec![30000, -30000, 5, 7],
            at: 0,
        };
        let mut pcm: Vec<u8> = [10000i16, -10000]
            .iter()
            .flat_map(|s| s.to_le_bytes())
            .collect();
        fade.mix(&mut pcm);
        assert_eq!(samples(&pcm), [i16::MAX, i16::MIN]);
        assert!(!fade.is_done());
        let mut rest = vec![0; 8];
        fade.mix(&mut rest);
        assert_eq!(samples(&rest), [5, 7, 0, 0]);
        assert!(fade.is_done());
    }

    #[test]
    fn a_seam_fade_at_the_streams_end_is_empty() {
        let mut decoder = Decoder::new(STEREO_16).unwrap();
        let mut all = vec![0; 20_000];
        assert_eq!(decoder.read(&mut all).unwrap(), 20_000);
        let mut fade = SeamFade::take(&mut decoder).unwrap();
        assert!(fade.is_done());
        let mut pcm = vec![1, 2, 3, 4];
        fade.mix(&mut pcm);
        assert_eq!(pcm, [1, 2, 3, 4]);
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
    fn narrow_samples_scale_up_and_16_bit_ones_pass_exactly() {
        let mut dither = Dither::new();
        assert_eq!(to_16_bits(-128, 8, None, &mut dither), -32768);
        assert_eq!(to_16_bits(127, 8, None, &mut dither), 32512);
        assert_eq!(to_16_bits(1234, 16, None, &mut dither), 1234);
        // No dither was drawn.
        assert_eq!(dither.0, Dither::new().0);
    }

    #[test]
    fn full_scale_stays_in_range_with_dither_and_gain() {
        let mut dither = Dither::new();
        for _ in 0..1000 {
            assert_eq!(to_16_bits(0x7F_FFFF, 24, None, &mut dither), 0x7FFF);
            // Dither can lift the lowest value by one before it rounds.
            assert!((-0x8000..=-0x7FFF).contains(&to_16_bits(-0x80_0000, 24, None, &mut dither)));
            assert_eq!(to_16_bits(30000, 16, Some(2.0), &mut dither), 0x7FFF);
            assert_eq!(to_16_bits(-30000, 16, Some(2.0), &mut dither), -0x8000);
        }
    }

    #[test]
    fn dither_is_triangular_and_has_no_bias() {
        let mut dither = Dither::new();
        let draws: Vec<f64> = (0..100_000).map(|_| dither.triangular()).collect();
        assert!(draws.iter().all(|d| (-1.0..1.0).contains(d)));
        let mean = draws.iter().sum::<f64>() / draws.len() as f64;
        let variance = draws.iter().map(|d| (d - mean).powi(2)).sum::<f64>() / draws.len() as f64;
        // A triangular distribution over (-1, 1) has variance 1/6.
        assert!(mean.abs() < 0.01, "mean {mean}");
        assert!((variance - 1.0 / 6.0).abs() < 0.01, "variance {variance}");
    }
}
