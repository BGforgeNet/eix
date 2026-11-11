//! VertexLighting: 3DfpFPU.dll's per-vertex lighting, x87 code run for every vertex of every figure each frame, runs
//! as SSE code four vertices at a time, with the same results. The game loads the library in
//! `CTransformPipeline::TransformPipelineConstruct` 0x4F0B90 (`LoadLibraryA` at 0x4F0C92); right after that, before
//! any figure is drawn, ours points the pipeline's virtual method (slot 40 of its vtable, the only way the code is
//! reached) at its own, when the library is the build the game ships. The arithmetic is core's
//! ([`eix_core::vertex_lighting`]) on the engine's SSE lanes ([`eix_engine::sse`]), at the precision the x87 control
//! word sets: 24 bits under Windows' Direct3D (the game asks for `DDSCL_FPUSETUP`), 53 under Wine; any
//! other setting runs the original's code.
#![expect(
    unsafe_code,
    reason = "reads the library's code and the game's vertex arrays, and calls the original method"
)]

use std::ops::{Add, Mul, Sub};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use eix_core::vertex_lighting::{Directional, Lanes, PointLight, vertices};
use eix_engine::group::{self, Outcome, SiteWrite};
use eix_engine::hook::{self, Context, Mode};
use eix_engine::sse::{F32x4, F64x4, Mask32, Mask64};
use eix_engine::{exe, patch, x87};
use eix_sites::Site;

use crate::groups::{BuildError, Labelled};
use crate::log;
use crate::setup::Setup;

/// `mov esi, [ebp-0x18]; mov [esi+0x10], eax`: EAX holds what `LoadLibraryA` returned for the transform library.
const LOADED: Site = eix_sites::C_TRANSFORM_PIPELINE_TRANSFORM_PIPELINE_CONSTRUCT__108;
const LOADED_BYTES: [u8; 6] = [0x8B, 0x75, 0xE8, 0x89, 0x46, 0x10];

/// The library's build: its image size and link time stamp (2000-01-25).
const IMAGE_SIZE: u32 = 0x2_7000;
const TIME_STAMP: u32 = 0x388D_B4F5;
/// The lighting method, from its entry to its `ret 0x20`.
const METHOD: usize = 0xD910;
const METHOD_LEN: usize = 0x5D5;
/// The method's absolute operands (offset in the method, the RVA it addresses), the only relocated words in it.
const OPERANDS: [(usize, usize); 7] = [
    (0x285, 0x2_3418),
    (0x28B, 0x2_341C),
    (0x2CE, 0x2_3428),
    (0x44C, 0x2_3420),
    (0x4A5, 0x2_342C),
    (0x4FE, 0x2_3434),
    (0x507, 0x2_3438),
];
/// The method's first instructions, `push ebx; mov ebx, esp; sub esp, ..`: the five bytes another module's `jmp`
/// takes (Universal Mod's um-engine.dll writes one from its own thread, at a time nothing orders against ours).
const ENTRY: [u8; 5] = [0x53, 0x8B, 0xDC, 0x83, 0xEC];
/// FNV-1a of the method's bytes with those operands and the entry zeroed: the code core's arithmetic follows. The
/// entry is left out so a `jmp` written there does not stop ours: the slot is swapped, so the method is not reached.
const METHOD_FNV: u64 = 0xAD21_20C0_87EB_37E2;
/// The constants the operands address (1.0, 0.0, 255.0, 1.0, 1.0, 255.0, 1.0, 255.0, 255.0), in `.data1`, which no
/// code of the library writes.
const CONSTANTS: usize = 0x2_3418;
const CONSTANT_BYTES: [u8; 36] = [
    0x00, 0x00, 0x80, 0x3F, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x7F, 0x43, 0x00, 0x00, 0x80, 0x3F,
    0x00, 0x00, 0x80, 0x3F, 0x00, 0x00, 0x7F, 0x43, 0x00, 0x00, 0x80, 0x3F, 0x00, 0x00, 0x7F, 0x43,
    0x00, 0x00, 0x7F, 0x43,
];
/// The method's vtable slot (slot 40 of the vtable at RVA 0x1F220).
const SLOT: usize = 0x1_F2C0;

/// The pipeline: the directional light's block, and the lowest red, green and blue.
const PIPELINE_DIRECTIONAL: usize = 0x04;
const PIPELINE_FLOOR: usize = 0x1C;
/// In the directional light's block: the direction, the colour, the alpha.
const DIRECTIONAL_DIRECTION: usize = 0x108;
const DIRECTIONAL_COLOUR: usize = 0x114;
const DIRECTIONAL_ALPHA: usize = 0x120;
/// In a point light: its position, its fall-off and its colour.
const LIGHT_POSITION: usize = 0x28;
const LIGHT_FALLOFF: usize = 0x104;
const LIGHT_COLOUR: usize = 0x114;
/// Positions and normals are blocks of 4 vertices, x, y and z each 4 floats: a vertex's floats are 0x10 apart.
const BLOCK: usize = 0x40;
const COMPONENT: usize = 0x10;
/// How many vertices are lit side by side: core's lanes, an SSE register's f32.
const LANES: usize = 4;
/// How many point lights a call keeps on the stack; more go to the heap.
const STACK_LIGHTS: usize = 16;
const NO_LIGHT: PointLight = PointLight {
    position: [0.0; 3],
    falloff: 0.0,
    colour: [0.0; 3],
};

/// Core's [`Lanes`] at the x87's 24-bit setting: four `f32` in an SSE register.
#[derive(Debug, Clone, Copy)]
struct Single(F32x4);

/// Core's [`Lanes`] at the x87's 53-bit setting: four `f64` in two SSE registers.
#[derive(Debug, Clone, Copy)]
struct Double(F64x4);

macro_rules! ops {
    ($lanes:ident) => {
        impl Add for $lanes {
            type Output = Self;
            fn add(self, o: Self) -> Self {
                Self(self.0 + o.0)
            }
        }

        impl Sub for $lanes {
            type Output = Self;
            fn sub(self, o: Self) -> Self {
                Self(self.0 - o.0)
            }
        }

        impl Mul for $lanes {
            type Output = Self;
            fn mul(self, o: Self) -> Self {
                Self(self.0 * o.0)
            }
        }
    };
}

ops!(Single);
ops!(Double);

impl Lanes for Single {
    type Mask = Mask32;
    fn splat(x: f32) -> Self {
        Self(F32x4::splat(x))
    }
    fn load(x: [f32; 4]) -> Self {
        Self(F32x4::from_array(x))
    }
    fn recip(self) -> Self {
        Self(F32x4::splat(1.0) / self.0)
    }
    fn sqrt(self) -> Self {
        Self(self.0.sqrt())
    }
    fn single(self) -> Self {
        self
    }
    fn floats(self) -> [f32; 4] {
        self.0.to_array()
    }
    fn at_least(self, other: Self) -> Mask32 {
        self.0.at_least(other.0)
    }
    fn any(mask: Mask32) -> bool {
        mask.any()
    }
    fn select(mask: Mask32, yes: Self, no: Self) -> Self {
        Self(F32x4::select(mask, yes.0, no.0))
    }
}

impl Lanes for Double {
    type Mask = Mask64;
    fn splat(x: f32) -> Self {
        Self(F64x4::splat(f64::from(x)))
    }
    fn load(x: [f32; 4]) -> Self {
        Self(F32x4::from_array(x).widen())
    }
    fn recip(self) -> Self {
        Self(F64x4::splat(1.0) / self.0)
    }
    fn sqrt(self) -> Self {
        Self(self.0.sqrt())
    }
    fn single(self) -> Self {
        Self(self.0.narrow().widen())
    }
    fn floats(self) -> [f32; 4] {
        self.0.narrow().to_array()
    }
    fn at_least(self, other: Self) -> Mask64 {
        self.0.at_least(other.0)
    }
    fn any(mask: Mask64) -> bool {
        mask.any()
    }
    fn select(mask: Mask64, yes: Self, no: Self) -> Self {
        Self(F64x4::select(mask, yes.0, no.0))
    }
}

/// `void __thiscall Light(pipeline, positions, normals, out, indices, count, lights, light_count, ambient)`: `out`
/// gets an ARGB colour per vertex; `indices` holds a position index and a normal index (u16 each) per vertex.
type Method = unsafe extern "thiscall" fn(
    *const u8,
    *const u8,
    *const u8,
    *mut u32,
    *const u16,
    i32,
    *const *const u8,
    i32,
    *const f32,
);

/// The original method's address in the loaded library.
static ORIGINAL: AtomicUsize = AtomicUsize::new(0);
/// Whether a call under another control word has been logged.
static OTHER_LOGGED: AtomicBool = AtomicBool::new(false);

/// The float `offset` bytes from `base`.
///
/// # Safety
/// The float must be readable.
unsafe fn float_at(base: *const u8, offset: usize) -> f32 {
    // SAFETY: the caller guarantees the float.
    unsafe {
        base.wrapping_byte_add(offset)
            .cast::<f32>()
            .read_unaligned()
    }
}

/// Three floats `stride` bytes apart, the first `offset` bytes from `base`.
///
/// # Safety
/// The three floats must be readable.
unsafe fn floats(base: *const u8, offset: usize, stride: usize) -> [f32; 3] {
    // SAFETY: the caller guarantees the floats.
    let x = unsafe { float_at(base, offset) };
    // SAFETY: as above.
    let y = unsafe { float_at(base, offset + stride) };
    // SAFETY: as above.
    let z = unsafe { float_at(base, offset + 2 * stride) };
    [x, y, z]
}

/// A vertex's x, y and z in a positions or normals array.
///
/// # Safety
/// `blocks` must hold the block of vertex `index`.
unsafe fn block_vertex(blocks: *const u8, index: u16) -> [f32; 3] {
    let index = usize::from(index);
    // SAFETY: the caller guarantees the block.
    unsafe { floats(blocks, index / 4 * BLOCK + index % 4 * 4, COMPONENT) }
}

/// The method's work at the precision `L` stands for.
///
/// # Safety
/// The arguments must be the method's, as the game passes them.
#[expect(clippy::too_many_arguments, reason = "the method's own arguments")]
unsafe fn light_at<L: Lanes>(
    pipeline: *const u8,
    positions: *const u8,
    normals: *const u8,
    out: *mut u32,
    indices: *const u16,
    count: i32,
    lights: *const *const u8,
    light_count: i32,
    ambient: *const f32,
) {
    // SAFETY: the pipeline holds the directional light's block.
    let block = unsafe {
        pipeline
            .wrapping_byte_add(PIPELINE_DIRECTIONAL)
            .cast::<*const u8>()
            .read_unaligned()
    };
    let directional = Directional {
        // SAFETY: the block's direction, colour and alpha.
        direction: unsafe { floats(block, DIRECTIONAL_DIRECTION, 4) },
        // SAFETY: as above.
        colour: unsafe { floats(block, DIRECTIONAL_COLOUR, 4) },
        // SAFETY: as above.
        alpha: unsafe { float_at(block, DIRECTIONAL_ALPHA) },
    };
    // SAFETY: the pipeline's three floors.
    let floor = unsafe { floats(pipeline, PIPELINE_FLOOR, 4) };
    // SAFETY: the ambient light's three floats.
    let ambient = unsafe { floats(ambient.cast(), 0, 4) };
    // The original reads every light again for each vertex; nothing writes them meanwhile, so once is the same.
    let read = |i: usize| {
        // SAFETY: `lights` holds `light_count` light pointers.
        let light = unsafe { lights.wrapping_add(i).read() }.cast::<u8>();
        PointLight {
            // SAFETY: the light's fields.
            position: unsafe { floats(light, LIGHT_POSITION, 4) },
            // SAFETY: as above.
            falloff: unsafe { float_at(light, LIGHT_FALLOFF) },
            // SAFETY: as above.
            colour: unsafe { floats(light, LIGHT_COLOUR, 4) },
        }
    };
    let in_use = usize::try_from(light_count).unwrap_or(0);
    // Up to STACK_LIGHTS on the stack: the method runs once per figure part each frame.
    let mut stack = [NO_LIGHT; STACK_LIGHTS];
    let heap: Vec<PointLight>;
    let point_lights: &[PointLight] = if in_use <= STACK_LIGHTS {
        for (i, slot) in stack[..in_use].iter_mut().enumerate() {
            *slot = read(i);
        }
        &stack[..in_use]
    } else {
        heap = (0..in_use).map(read).collect();
        &heap
    };
    let count = usize::try_from(count).unwrap_or(0);
    // LANES vertices at a time; the last group repeats its first vertex where it runs short, and writes only its own.
    for first in (0..count).step_by(LANES) {
        let mut p = [[0.0; 3]; LANES];
        let mut n = [[0.0; 3]; LANES];
        for lane in 0..LANES {
            let i = if first + lane < count {
                first + lane
            } else {
                first
            };
            // SAFETY: `indices` holds two u16 per vertex.
            let position = unsafe { indices.wrapping_add(2 * i).read() };
            // SAFETY: as above.
            let normal = unsafe { indices.wrapping_add(2 * i + 1).read() };
            // SAFETY: the arrays hold the vertices the indices name.
            p[lane] = unsafe { block_vertex(positions, position) };
            // SAFETY: as above.
            n[lane] = unsafe { block_vertex(normals, normal) };
        }
        let colours = vertices::<L>(&directional, floor, &n, &p, point_lights, ambient);
        for (lane, colour) in colours.iter().enumerate().take(count - first) {
            // SAFETY: `out` has a colour per vertex, and `first + lane < count`.
            unsafe { out.wrapping_add(first + lane).write(*colour) };
        }
    }
}

/// Stands in for the method: core's arithmetic at the x87's precision, or the original's code under a setting core
/// does not reproduce (64-bit precision, another rounding).
unsafe extern "thiscall" fn light(
    pipeline: *const u8,
    positions: *const u8,
    normals: *const u8,
    out: *mut u32,
    indices: *const u16,
    count: i32,
    lights: *const *const u8,
    light_count: i32,
    ambient: *const f32,
) {
    let word = x87::control_word();
    let (precision, rounding) = ((word >> 8) & 3, (word >> 10) & 3);
    match (precision, rounding) {
        // SAFETY: the game's arguments.
        (0, 0) => unsafe {
            light_at::<Single>(
                pipeline,
                positions,
                normals,
                out,
                indices,
                count,
                lights,
                light_count,
                ambient,
            );
        },
        // SAFETY: as above.
        (2, 0) => unsafe {
            light_at::<Double>(
                pipeline,
                positions,
                normals,
                out,
                indices,
                count,
                lights,
                light_count,
                ambient,
            );
        },
        _ => {
            if !OTHER_LOGGED.swap(true, Ordering::Relaxed) {
                log::info(
                    "fixes",
                    &format!("x87 control word {word:#06X}: 3DfpFPU.dll's own lighting runs"),
                    "VertexLighting",
                );
            }
            // SAFETY: ORIGINAL holds the method `install` replaced, of this type.
            let original: Method = unsafe { hook::as_fn_at(ORIGINAL.load(Ordering::SeqCst)) };
            // SAFETY: the game's arguments.
            unsafe {
                original(
                    pipeline,
                    positions,
                    normals,
                    out,
                    indices,
                    count,
                    lights,
                    light_count,
                    ambient,
                );
            }
        }
    }
}

/// Why the library at `module` is not the build ours stands in for, if it is not.
///
/// # Safety
/// `module` must be the base of a loaded module.
unsafe fn other_build(module: usize) -> Option<String> {
    // SAFETY: the caller guarantees the module.
    let id = match unsafe { exe::module(module) } {
        Ok(id) => id,
        Err(e) => return Some(e),
    };
    if (id.size_of_image, id.time_date_stamp) != (IMAGE_SIZE, TIME_STAMP) {
        return Some(format!(
            "another build of the library (image size {:#x}, time stamp {:#010x})",
            id.size_of_image, id.time_date_stamp
        ));
    }
    // SAFETY: the method lies inside the image, whose size was checked.
    let mut code = unsafe { patch::read(Site::of(module + METHOD), METHOD_LEN) };
    for (offset, rva) in OPERANDS {
        let operand = &mut code[offset..offset + 4];
        if *operand != ((module + rva) as u32).to_le_bytes() {
            return Some(format!(
                "the method's operand at +{offset:#x} does not address +{rva:#x}"
            ));
        }
        operand.fill(0);
    }
    let entry = &mut code[..ENTRY.len()];
    if *entry != ENTRY && entry[0] != 0xE9 {
        return Some(format!("the method's entry differs ({entry:02X?})"));
    }
    entry.fill(0);
    let fnv = code.iter().fold(0xCBF2_9CE4_8422_2325_u64, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01B3)
    });
    if fnv != METHOD_FNV {
        return Some(format!("the method's code differs (FNV {fnv:#018x})"));
    }
    // SAFETY: the constants lie inside the image.
    unsafe { patch::verify(Site::of(module + CONSTANTS), &CONSTANT_BYTES) }
        .err()
        .map(|e| format!("the method's constants differ: {e}"))
}

/// After `LoadLibraryA` of the transform library: its lighting method becomes ours.
extern "C" fn loaded(_: usize, ctx: &mut Context) {
    let module = ctx.eax as usize;
    if module == 0 {
        return;
    }
    // SAFETY: EAX is the handle LoadLibraryA returned, the base of the loaded library.
    if let Some(why) = unsafe { other_build(module) } {
        log::info(
            "fixes",
            &format!("3DfpFPU.dll's own lighting runs: {why}"),
            "VertexLighting",
        );
        return;
    }
    let (slot, method) = (module + SLOT, module + METHOD);
    ORIGINAL.store(method, Ordering::SeqCst);
    // SAFETY: the slot lies in the library's `.data`, and nothing calls the method before the pipeline is made,
    // further on in this function, on this thread.
    let report = unsafe {
        group::apply("VertexLighting", true, || {
            Ok(vec![SiteWrite::pointer(
                Site::of(slot),
                method,
                light as *const () as usize,
            )])
        })
    };
    match report.outcome {
        Outcome::Applied => log::info(
            "fixes",
            "3DfpFPU.dll's lighting replaced",
            &format!("VertexLighting 0x{slot:08X}+4"),
        ),
        Outcome::Disabled | Outcome::Failed { .. } => {
            log::error("fixes", &report.to_string(), "VertexLighting");
        }
    }
}

pub(super) fn build(_: &Setup) -> Result<Vec<Labelled>, BuildError> {
    Ok(vec![Labelled {
        label: "VertexLighting #0".to_owned(),
        write: hook::context_hook(LOADED, &LOADED_BYTES, Mode::Before, loaded, 0)?,
    }])
}

#[cfg(test)]
mod tests {
    use super::*;

    use eix_engine::dll::Dll;

    /// Held by each test that loads the game's library: one test writes into its code, the others run it, and every
    /// load of it in this process is the same module.
    static LIBRARY: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// xorshift32: the same scenes on every run.
    struct Rng(u32);

    impl Rng {
        fn next(&mut self) -> u32 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 17;
            self.0 ^= self.0 << 5;
            self.0
        }

        /// A float from `lo` to `hi`, and now and then one of the values the comparisons turn on.
        fn float(&mut self, lo: f32, hi: f32) -> f32 {
            match self.next() % 32 {
                0 => 0.0,
                1 => -0.0,
                2 => 1.0,
                3 => lo,
                _ => lo + (hi - lo) * (self.next() % 1_000_000) as f32 / 1_000_000.0,
            }
        }

        fn floats(&mut self, lo: f32, hi: f32) -> [f32; 3] {
            [self.float(lo, hi), self.float(lo, hi), self.float(lo, hi)]
        }
    }

    fn put(buffer: &mut [u8], at: usize, values: &[f32]) {
        for (i, v) in values.iter().enumerate() {
            buffer[at + 4 * i..at + 4 * i + 4].copy_from_slice(&v.to_le_bytes());
        }
    }

    const VERTICES: usize = 64;
    const LIGHTS: usize = 6;

    /// One scene's inputs, laid out as the game's.
    struct Scene {
        pipeline: Vec<u8>,
        directional: Vec<u8>,
        positions: Vec<u8>,
        normals: Vec<u8>,
        indices: Vec<u16>,
        lights: Vec<Vec<u8>>,
        ambient: [f32; 3],
    }

    fn scene(rng: &mut Rng) -> Scene {
        let mut directional = vec![0u8; 0x124];
        put(
            &mut directional,
            DIRECTIONAL_DIRECTION,
            &rng.floats(-1.0, 1.0),
        );
        put(&mut directional, DIRECTIONAL_COLOUR, &rng.floats(0.0, 1.5));
        put(&mut directional, DIRECTIONAL_ALPHA, &[rng.float(0.0, 1.0)]);
        let mut pipeline = vec![0u8; 0x28];
        put(&mut pipeline, PIPELINE_FLOOR, &rng.floats(-0.2, 0.4));
        let mut positions = vec![0u8; VERTICES / 4 * BLOCK];
        let mut normals = vec![0u8; VERTICES / 4 * BLOCK];
        for f in positions.as_chunks_mut::<4>().0 {
            *f = rng.float(-20.0, 20.0).to_le_bytes();
        }
        for f in normals.as_chunks_mut::<4>().0 {
            *f = rng.float(-1.0, 1.0).to_le_bytes();
        }
        let indices = (0..2 * VERTICES)
            .map(|_| (rng.next() % VERTICES as u32) as u16)
            .collect();
        let lights = (0..LIGHTS)
            .map(|_| {
                let mut light = vec![0u8; 0x120];
                put(&mut light, LIGHT_POSITION, &rng.floats(-20.0, 20.0));
                put(&mut light, LIGHT_FALLOFF, &[rng.float(0.0, 0.01)]);
                put(&mut light, LIGHT_COLOUR, &rng.floats(0.0, 1.5));
                light
            })
            .collect();
        let ambient = rng.floats(-0.1, 0.4);
        Scene {
            pipeline,
            directional,
            positions,
            normals,
            indices,
            lights,
            ambient,
        }
    }

    /// The colours `method` gives for `scene`.
    fn colours(method: Method, scene: &mut Scene) -> Vec<u32> {
        let block = scene.directional.as_ptr() as usize as u32;
        scene.pipeline[PIPELINE_DIRECTIONAL..PIPELINE_DIRECTIONAL + 4]
            .copy_from_slice(&block.to_le_bytes());
        let lights: Vec<*const u8> = scene.lights.iter().map(|l| l.as_ptr()).collect();
        let mut out = vec![0u32; scene.indices.len() / 2];
        // SAFETY: every array has what the method reads: a colour per vertex, the indices' blocks, the lights.
        unsafe {
            method(
                scene.pipeline.as_ptr(),
                scene.positions.as_ptr(),
                scene.normals.as_ptr(),
                out.as_mut_ptr(),
                scene.indices.as_ptr(),
                out.len() as i32,
                lights.as_ptr(),
                lights.len() as i32,
                scene.ambient.as_ptr(),
            );
        }
        out
    }

    /// Vertices whose colour differs at 24 and 53 bits (a channel lands on a rounding tie at one of them), found by a
    /// search over random inputs: the directional light's direction, colour and alpha, the normal, the position, one
    /// point light's position, fall-off and colour, the ambient light, and the colour at 24 and at 53 bits. The
    /// floors are -1.
    const TIES: [Tie; 6] = [
        (
            [-0.832502, 0.37378204, 0.22801006],
            [1.303251, 0.297831, 0.9621285],
            0.483206,
            [0.66443205, -0.814452, -0.333156],
            [1.9506302, -0.6199498, 1.94208],
            [-3.33077, 2.3955598, -3.15854],
            0.00512496,
            [1.131666, 1.422027, 1.485336],
            [0.1308507, 0.16979732, 0.09933901],
            0xFFFF_FF90,
            0xFFFF_FF8F,
        ),
        (
            [0.22332, 0.70134795, -0.16760999],
            [1.315311, 1.485771, 0.7820565],
            0.186833,
            [-0.08416802, 0.970644, 0.746328],
            [-2.00367, 0.4477601, -4.0027],
            [4.5128403, 0.5601802, -4.93843],
            0.0159641,
            [0.8009265, 0.163416, 1.180326],
            [0.04875, 0.0084693, 0.21303812],
            0x30C1_CEA1,
            0x30C0_CEA1,
        ),
        (
            [-0.369344, 0.16540802, 0.926366],
            [0.29295, 1.081872, 0.0361935],
            0.718615,
            [0.04412198, 0.06549394, -0.579528],
            [-3.95532, 2.6925101, -0.42366982],
            [3.0707703, 2.6227198, 3.8217201],
            0.015476599,
            [0.273702, 0.1246455, 0.995031],
            [0.0439245, 0.1926009, 0.0608478],
            0xFFFF_9C0B,
            0xFFFF_9B0B,
        ),
        (
            [0.18262196, 0.77006197, -0.455638],
            [0.8471745, 1.3330455, 0.757695],
            0.489743,
            [-0.70583403, -0.655204, 0.98992],
            [-0.69033, 4.51408, 2.9292302],
            [-4.5104, -3.57309, 2.46381],
            0.011442239,
            [0.9147015, 0.9261225, 1.448481],
            [0.19753441, 0.2674908, 0.0932217],
            0x7D43_5531,
            0x7D42_5531,
        ),
        (
            [0.71243, 0.650524, 0.38633],
            [1.217931, 1.4440575, 0.8042355],
            0.574251,
            [-0.431732, 0.613942, 0.448668],
            [-1.44099, -3.96855, 4.0842505],
            [-3.86201, -2.00772, 2.4447598],
            0.0097845,
            [1.3782074, 0.593286, 1.017225],
            [0.047481302, 0.0867492, 0.194157],
            0x9291_7893,
            0x9291_7894,
        ),
        (
            [-0.645244, 0.45921803, -0.18345398],
            [0.927339, 0.774375, 0.010176],
            0.681476,
            [-0.891302, -0.002224028, -0.37923998],
            [1.7219, -2.62051, 1.0407901],
            [0.21546984, 3.4973402, 0.025710106],
            0.01825956,
            [1.376751, 0.1680765, 0.8663685],
            [0.2775054, 0.2839194, 0.012546601],
            0xAEDF_C812,
            0xAEDF_C712,
        ),
    ];

    /// A [`TIES`] row.
    type Tie = (
        [f32; 3],
        [f32; 3],
        f32,
        [f32; 3],
        [f32; 3],
        [f32; 3],
        f32,
        [f32; 3],
        [f32; 3],
        u32,
        u32,
    );

    /// A one-vertex, one-light scene of a [`TIES`] row.
    fn tie_scene(t: &Tie) -> Scene {
        let (
            direction,
            colour,
            alpha,
            normal,
            position,
            light_position,
            falloff,
            light_colour,
            ambient,
            _,
            _,
        ) = *t;
        let mut directional = vec![0u8; 0x124];
        put(&mut directional, DIRECTIONAL_DIRECTION, &direction);
        put(&mut directional, DIRECTIONAL_COLOUR, &colour);
        put(&mut directional, DIRECTIONAL_ALPHA, &[alpha]);
        let mut pipeline = vec![0u8; 0x28];
        put(&mut pipeline, PIPELINE_FLOOR, &[-1.0; 3]);
        // Vertex 0 of a block: x, y and z COMPONENT bytes apart.
        let mut positions = vec![0u8; BLOCK];
        let mut normals = vec![0u8; BLOCK];
        for (k, (p, n)) in position.iter().zip(normal).enumerate() {
            put(&mut positions, k * COMPONENT, &[*p]);
            put(&mut normals, k * COMPONENT, &[n]);
        }
        let mut light = vec![0u8; 0x120];
        put(&mut light, LIGHT_POSITION, &light_position);
        put(&mut light, LIGHT_FALLOFF, &[falloff]);
        put(&mut light, LIGHT_COLOUR, &light_colour);
        Scene {
            pipeline,
            directional,
            positions,
            normals,
            indices: vec![0, 0],
            lights: vec![light],
            ambient,
        }
    }

    #[test]
    #[ignore = "a timing, not a check: EIX_GAME_DIR=<the game folder>, run with --nocapture"]
    fn timing() {
        let _only = LIBRARY
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let dir = std::env::var("EIX_GAME_DIR").expect("EIX_GAME_DIR names the game folder");
        let path: Vec<u16> = format!("{dir}\\3dfpfpu.dll").encode_utf16().collect();
        let dll = Dll::load(&path).expect("3dfpfpu.dll loads");
        // SAFETY: the method's address in the loaded library, of this type.
        let original: Method = unsafe { hook::as_fn_at(dll.handle() + METHOD) };
        let mut rng = Rng(7);
        let mut scenes: Vec<Scene> = (0..200).map(|_| scene(&mut rng)).collect();
        for precision in [0, 2] {
            for (name, method) in [("original", original), ("ours", light as Method)] {
                // The best of five: other load on the machine only adds.
                let best = (0..5)
                    .map(|_| {
                        let start = std::time::Instant::now();
                        x87::with_precision(precision, || {
                            for _ in 0..50 {
                                for s in &mut scenes {
                                    std::hint::black_box(colours(method, s));
                                }
                            }
                        });
                        start.elapsed()
                    })
                    .min()
                    .unwrap_or_default();
                let per = best.as_nanos() as f64 / (50.0 * 200.0 * VERTICES as f64);
                eprintln!(
                    "precision field {precision}: {name} {per:.1} ns per vertex ({LIGHTS} lights)"
                );
            }
        }
    }

    #[test]
    #[ignore = "needs the game's 3dfpfpu.dll: EIX_GAME_DIR=<the game folder, as a Windows path>"]
    fn another_modules_jump_at_the_entry_still_takes_ours() {
        let _only = LIBRARY
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let dir = std::env::var("EIX_GAME_DIR").expect("EIX_GAME_DIR names the game folder");
        let path: Vec<u16> = format!("{dir}\\3dfpfpu.dll").encode_utf16().collect();
        let dll = Dll::load(&path).expect("3dfpfpu.dll loads");
        let method = Site::of(dll.handle() + METHOD);
        let jump = patch::jmp_rel32(method, method.here() + 0x1000);
        // SAFETY: the method's entry in the loaded library, which no other test runs while LIBRARY is held.
        unsafe { patch::write(method, &ENTRY, &jump) }.expect("the jump is written");
        // SAFETY: the library is loaded.
        let with_jump = unsafe { other_build(dll.handle()) };
        // SAFETY: as above.
        unsafe { patch::write(method, &jump, &[0xCC; 5]) }.expect("the entry is overwritten");
        // SAFETY: as above.
        let overwritten = unsafe { other_build(dll.handle()) };
        // SAFETY: as above; the entry goes back before the other test may call the method.
        unsafe { patch::write(method, &[0xCC; 5], &ENTRY) }.expect("the entry is restored");
        assert_eq!(with_jump, None);
        assert!(overwritten.is_some_and(|why| why.contains("entry")));
    }

    #[test]
    #[ignore = "needs the game's 3dfpfpu.dll: EIX_GAME_DIR=<the game folder, as a Windows path>"]
    fn ours_gives_the_originals_colours_at_every_precision() {
        let _only = LIBRARY
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let dir = std::env::var("EIX_GAME_DIR").expect("EIX_GAME_DIR names the game folder");
        let path: Vec<u16> = format!("{dir}\\3dfpfpu.dll").encode_utf16().collect();
        let dll = Dll::load(&path).expect("3dfpfpu.dll loads");
        let module = dll.handle();
        // SAFETY: the library is loaded at `module`.
        assert_eq!(unsafe { other_build(module) }, None);
        ORIGINAL.store(module + METHOD, Ordering::SeqCst);
        // SAFETY: the method's address in the loaded library, of this type.
        let original: Method = unsafe { hook::as_fn_at(module + METHOD) };
        let ours: Method = light;
        // The precisions give different colours only where a channel lands on a rounding tie, about one vertex in
        // 140 000 of the random scenes: these show that each precision's path is the one that matches.
        for (i, t) in TIES.iter().enumerate() {
            let mut s = tie_scene(t);
            for (precision, want) in [(0, t.9), (2, t.10)] {
                let (theirs, mine) = x87::with_precision(precision, || {
                    (colours(original, &mut s), colours(ours, &mut s))
                });
                assert_eq!(
                    (theirs[0], mine[0]),
                    (want, want),
                    "tie {i}, precision field {precision}"
                );
            }
        }
        let mut rng = Rng(0x2545_F491);
        for round in 0..2000 {
            let mut s = scene(&mut rng);
            // 24, 53 and 64 bits; at 64 ours runs the original.
            for precision in [0, 2, 3] {
                let (theirs, mine) = x87::with_precision(precision, || {
                    (colours(original, &mut s), colours(ours, &mut s))
                });
                assert_eq!(theirs, mine, "scene {round}, precision field {precision}");
            }
        }
    }
}
