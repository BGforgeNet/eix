//! The one Lua state of the addon: creation, the main script, protected calls and the fatal error report.

use std::rc::Rc;

use eix_core::options::AddonSection;
use mlua::{Function, IntoLuaMulti, Lua, MultiValue, Value};

use crate::game::Game;
use crate::{functions, handler, state};

/// Shows a fatal Lua error and ends the game. The message is the error's text (with its traceback) as the bytes Lua
/// holds, in the ANSI code page; it is shown literally.
pub type FatalReport = Box<dyn Fn(&[u8])>;

/// The fatal report of the original: a modal error box titled `title` with the message, then `ExitProcess(1)`.
///
/// Diverges on purpose: the message is shown as text, never used as a format string. It is decoded from
/// the ANSI code page, as the original converts it.
pub fn error_box_and_exit(title: String) -> FatalReport {
    Box::new(move |message: &[u8]| {
        let text = String::from_utf16_lossy(&eix_engine::win::from_ansi(message));
        eix_engine::win::error_box(&title, text);
        eix_engine::win::exit_process(1)
    })
}

/// The Lua state with the 57 registered functions, and the calls into it.
pub struct Host {
    lua: Lua,
    /// LuaJIT's `xpcall` and `loadfile`, taken before any script can replace the globals, as the original calls
    /// the C API directly.
    xpcall: Function,
    loadfile: Function,
    handler: Function,
    /// The last traceback line, `xpcall`'s own frame: the original calls `lua_pcall` from C, so its tracebacks end
    /// at the called function, and the report drops this line.
    host_frame: Vec<u8>,
    fatal: FatalReport,
}

impl core::fmt::Debug for Host {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Host").finish_non_exhaustive()
    }
}

impl Host {
    /// Creates the state as addon.dll does at load, whatever the config says: the libraries and the 57 functions.
    pub fn new(game: Rc<dyn Game>, fatal: FatalReport) -> mlua::Result<Self> {
        let lua = state::new_state()?;
        functions::register_all(&lua, &game)?;
        let globals = lua.globals();
        let xpcall = globals.get("xpcall")?;
        let loadfile = globals.get("loadfile")?;
        let handler = handler::create(&lua)?;
        let host_frame = host_frame(&lua, &xpcall, &handler)?;
        Ok(Self {
            lua,
            xpcall,
            loadfile,
            handler,
            host_frame,
            fatal,
        })
    }

    /// The state, for what the binding does not wrap.
    pub fn lua(&self) -> &Lua {
        &self.lua
    }

    /// Runs the mod's main script when `EnableLuaScripts` is on: the config read calls this, at load and
    /// again for `addon moddir`, in the same state.
    ///
    /// `mod_dir` is the mod folder ai.reg is read from. `package.path` gains `<dir>?.lua;<dir>lua/?.lua`, with `<dir>`
    /// that folder ending in `/` and every `\` made `/`; the script is `<dir>` plus `LuaMainScriptPath` as read. The
    /// script may be LuaJIT bytecode. A load or run error goes to the fatal report.
    pub fn run_main_script(&self, addon: &AddonSection, mod_dir: &[u8]) {
        if addon.enable_lua_scripts == 0 {
            return;
        }
        let dir = script_dir(mod_dir);
        let chunk = [
            b"package.path = package.path .. \";".as_slice(),
            &dir,
            b"?.lua;",
            &dir,
            b"lua/?.lua\"",
        ]
        .concat();
        // The original ignores this chunk's result: a `"` in the folder breaks it and package.path stays unchanged.
        let _ = self.lua.load(chunk.as_slice()).exec();
        let path = [dir.as_slice(), &addon.lua_main_script_path].concat();
        let loaded = self
            .lua
            .create_string(&path)
            .and_then(|path| self.loadfile.call::<(Value, Value)>(path));
        match loaded {
            Ok((Value::Function(main), _)) => {
                self.protected_call(Value::Function(main), ());
            }
            // `loadfile` returns nil and the message, which the original reports without a traceback.
            Ok((_, message)) => self.report(&message),
            Err(e) => self.report_mlua(&e),
        }
    }

    /// Whether the global `name` is a function: a callback the script defines.
    pub fn is_function(&self, name: &str) -> bool {
        matches!(
            self.lua.globals().get::<Value>(name),
            Ok(Value::Function(_))
        )
    }

    /// Calls the global `name` with `args` as every callback does: looked up on each call, called through
    /// the protected-call helper, an error reported. Returns the first result, or the error value after an error,
    /// which the original reads the same way.
    pub(crate) fn call_global(&self, name: &str, args: impl IntoLuaMulti) -> Value {
        match self.lua.globals().get::<Value>(name) {
            Ok(f) => self.protected_call(f, args),
            Err(e) => {
                self.report_mlua(&e);
                Value::Nil
            }
        }
    }

    /// The protected-call helper and the report: the traceback handler, a full collection after an
    /// error, then the report unless the error value is nil.
    fn protected_call(&self, f: Value, args: impl IntoLuaMulti) -> Value {
        let call = args.into_lua_multi(&self.lua).and_then(|mut all| {
            all.push_front(Value::Function(self.handler.clone()));
            all.push_front(f);
            self.xpcall.call::<MultiValue>(all)
        });
        let mut results = match call {
            Ok(results) => results,
            Err(e) => {
                self.collect();
                self.report_mlua(&e);
                return Value::Nil;
            }
        };
        let ok = results.pop_front();
        let first = results.pop_front().unwrap_or(Value::Nil);
        if matches!(ok, Some(Value::Boolean(true))) {
            return first;
        }
        self.collect();
        if let Value::String(s) = &first {
            let bytes = s.as_bytes();
            if let Some(trimmed) = bytes.strip_suffix(self.host_frame.as_slice()) {
                (self.fatal)(trimmed);
                return first;
            }
        }
        self.report(&first);
        first
    }

    fn collect(&self) {
        // A failed collection changes nothing a script sees; the error being reported matters more.
        let _ = self.lua.gc_collect();
    }

    /// The report of an error value: nil is ignored (`error(nil)`); a string or number goes to the fatal report.
    fn report(&self, error: &Value) {
        if error.is_nil() {
            return;
        }
        // A value that is neither a string nor a number has no text: `lua_tolstring` gives none in the original.
        let message = if let Value::String(s) = error {
            s.as_bytes().to_vec()
        } else if matches!(error, Value::Number(_) | Value::Integer(_)) {
            self.lua
                .coerce_string(error.clone())
                .ok()
                .flatten()
                .map(|s| s.as_bytes().to_vec())
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        (self.fatal)(&message);
    }

    /// An error of the binding itself (out of memory, an erroring metamethod on `_G`).
    fn report_mlua(&self, error: &mlua::Error) {
        (self.fatal)(error.to_string().as_bytes());
    }
}

/// The mod folder as the script directory: a `\` appended when missing, then every `\` made `/`.
fn script_dir(mod_dir: &[u8]) -> Vec<u8> {
    let mut dir = mod_dir.to_vec();
    if dir.last() != Some(&b'\\') {
        dir.push(b'\\');
    }
    for b in &mut dir {
        if *b == b'\\' {
            *b = b'/';
        }
    }
    dir
}

/// The traceback line of `xpcall`'s frame (`\n\t[builtin#21]: at 0x...`, its address fixed for the state's life),
/// read off one probe error.
fn host_frame(lua: &Lua, xpcall: &Function, handler: &Function) -> mlua::Result<Vec<u8>> {
    let raise = lua.load("error('probe', 0)").into_function()?;
    let (_, traceback): (bool, mlua::LuaString) = xpcall.call((raise, handler.clone()))?;
    let bytes = traceback.as_bytes();
    let start = bytes
        .iter()
        .rposition(|&b| b == b'\n')
        .unwrap_or(bytes.len());
    Ok(bytes[start..].to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_dir_ends_in_a_slash_and_uses_slashes() {
        assert_eq!(script_dir(b"Mods\\inv"), b"Mods/inv/");
        assert_eq!(script_dir(b"C:\\Game\\Mods\\inv\\"), b"C:/Game/Mods/inv/");
        assert_eq!(script_dir(b"Mods/inv/"), b"Mods/inv//");
    }
}
