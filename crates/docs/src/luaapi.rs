//! The Lua reference: `docs/eix.lua`, a LuaLS definitions file, read into items and rendered as a page.
//!
//! The file's shape: `---@meta`, then sections. A line `-- Section: <title>` opens a section, and the plain comment
//! lines after it, up to a blank line, are its introduction. An item is a block of `---` lines (descriptions and
//! `---@` annotations) ended by a `function Name(args) end` line, or, for an `---@alias`, by a blank line.

use std::fmt::Write as _;

use crate::{DocsError, generated_note, push_wrapped};

/// The data file's name, as the page's note names it.
const DATA_FILE: &str = "docs/eix.lua";

/// The section of the functions addon.dll registers.
pub const FUNCTIONS_SECTION: &str = "Functions supplied by eix";

/// The section of the callbacks a mod defines.
pub const CALLBACKS_SECTION: &str = "Callbacks supplied by the mod";

/// What an item declares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Declares {
    /// A type alias: `---@alias Name Type`.
    Alias,
    /// A global function.
    Function {
        /// The parameter names, in order.
        params: Vec<String>,
    },
}

/// One documented item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// The item's name.
    pub name: String,
    /// What it declares.
    pub declares: Declares,
    /// The `---@` annotation lines, as written.
    pub annotations: Vec<String>,
    /// The declaration line of a function (`function Name(a, b) end`); empty for an alias.
    pub declaration: String,
    /// The description: paragraphs separated by a blank line.
    pub description: Vec<String>,
}

/// A section of the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    /// The title after `Section:`.
    pub title: String,
    /// The introduction, one paragraph.
    pub intro: String,
    /// The items, in file order.
    pub items: Vec<Item>,
}

/// The definitions file, read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LuaApi {
    /// The sections, in file order.
    pub sections: Vec<Section>,
}

/// An item's lines while the parser collects them.
#[derive(Default)]
struct Block {
    lines: Vec<String>,
}

impl LuaApi {
    /// Reads the definitions file.
    pub fn parse(text: &str) -> Result<Self, DocsError> {
        let mut sections: Vec<Section> = Vec::new();
        let mut block = Block::default();
        let mut in_intro = false;
        for (index, line) in text.lines().enumerate() {
            let at = |what: &str| DocsError::new(format!("{DATA_FILE}:{}: {what}", index + 1));
            if line.trim().is_empty() {
                in_intro = false;
                if !block.lines.is_empty() {
                    // A block ends at a blank line only when it declares a type.
                    let item = block.finish(None).map_err(|e| at(&e))?;
                    open(&mut sections).map_err(|e| at(&e))?.items.push(item);
                    block = Block::default();
                }
            } else if let Some(doc) = line.strip_prefix("---") {
                if index == 0 && doc == "@meta" {
                    continue;
                }
                block
                    .lines
                    .push(doc.strip_prefix(' ').unwrap_or(doc).to_owned());
            } else if let Some(comment) = line.strip_prefix("--") {
                let comment = comment.strip_prefix(' ').unwrap_or(comment);
                if let Some(title) = comment.strip_prefix("Section: ") {
                    sections.push(Section {
                        title: title.trim().to_owned(),
                        intro: String::new(),
                        items: Vec::new(),
                    });
                    in_intro = true;
                } else if in_intro {
                    let section = open(&mut sections).map_err(|e| at(&e))?;
                    if !section.intro.is_empty() {
                        section.intro.push(' ');
                    }
                    section.intro.push_str(comment.trim());
                } else if !sections.is_empty() {
                    return Err(at("a plain comment outside a section's introduction"));
                }
            } else {
                let declaration = line.trim_end();
                let item = block.finish(Some(declaration)).map_err(|e| at(&e))?;
                open(&mut sections).map_err(|e| at(&e))?.items.push(item);
                block = Block::default();
            }
        }
        if !block.lines.is_empty() {
            let item = block
                .finish(None)
                .map_err(|e| DocsError::new(format!("{DATA_FILE}: {e}")))?;
            open(&mut sections)
                .map_err(|e| DocsError::new(format!("{DATA_FILE}: {e}")))?
                .items
                .push(item);
        }
        Ok(Self { sections })
    }

    /// The names of the functions in a section, in file order.
    pub fn function_names(&self, section: &str) -> Vec<&str> {
        self.sections
            .iter()
            .filter(|s| s.title == section)
            .flat_map(|s| &s.items)
            .filter(|i| matches!(i.declares, Declares::Function { .. }))
            .map(|i| i.name.as_str())
            .collect()
    }

    /// The page, `docs/lua-reference.md`.
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str("# Lua reference\n\n");
        out.push_str(&generated_note(DATA_FILE));
        out.push_str("\n\n");
        let intro = "The Lua API of a mod's main script: the functions addon.dll registers before the script runs, \
            and the callbacks the script defines. [Lua](lua.md) says how the host runs the script. The declarations \
            below are annotations for the Lua language server (LuaLS); the same text is the definitions file \
            [eix.lua](eix.lua), in the `docs` folder of the release package. Add it to the server's workspace library \
            for completion and type checks.";
        push_wrapped(&mut out, intro);
        for section in &self.sections {
            let _ = write!(out, "\n## {}\n\n", section.title);
            push_wrapped(&mut out, &section.intro);
            for item in &section.items {
                out.push('\n');
                item.render(&mut out);
            }
        }
        out
    }
}

/// The section being filled.
fn open(sections: &mut [Section]) -> Result<&mut Section, String> {
    sections
        .last_mut()
        .ok_or_else(|| "an item before the first `-- Section:` line".to_owned())
}

impl Block {
    /// Builds the item from the collected lines and the declaration line that ended the block, if any.
    fn finish(&self, declaration: Option<&str>) -> Result<Item, String> {
        let mut annotations = Vec::new();
        let mut description: Vec<String> = Vec::new();
        let mut paragraph = String::new();
        let end_paragraph = |paragraph: &mut String, description: &mut Vec<String>| {
            if !paragraph.is_empty() {
                description.push(std::mem::take(paragraph));
            }
        };
        for line in &self.lines {
            if line.starts_with('@') {
                annotations.push(format!("---{line}"));
            } else if line.trim().is_empty() {
                end_paragraph(&mut paragraph, &mut description);
            } else {
                if !paragraph.is_empty() {
                    paragraph.push(' ');
                }
                paragraph.push_str(line.trim());
            }
        }
        end_paragraph(&mut paragraph, &mut description);

        let Some(declaration) = declaration else {
            let alias = annotations
                .iter()
                .find_map(|a| a.strip_prefix("---@alias "))
                .ok_or("a block of `---` lines ends without a `function` line or an `---@alias`")?;
            let name = alias
                .split_whitespace()
                .next()
                .ok_or("an `---@alias` without a name")?;
            return Ok(Item {
                name: name.to_owned(),
                declares: Declares::Alias,
                annotations: vec![format!("---@alias {alias}")],
                declaration: String::new(),
                description,
            });
        };

        let (name, params) = parse_function(declaration)?;
        let documented: Vec<&str> = annotations
            .iter()
            .filter_map(|a| a.strip_prefix("---@param "))
            .filter_map(|a| a.split_whitespace().next())
            .collect();
        if documented != params {
            return Err(format!(
                "{name}: the `---@param` names {documented:?} differ from the parameters {params:?}"
            ));
        }
        if description.is_empty() {
            return Err(format!("{name}: no description"));
        }
        Ok(Item {
            name,
            declares: Declares::Function { params },
            annotations,
            declaration: declaration.to_owned(),
            description,
        })
    }
}

/// The name and parameters of a `function Name(a, b) end` line.
fn parse_function(line: &str) -> Result<(String, Vec<String>), String> {
    let rest = line
        .strip_prefix("function ")
        .and_then(|r| r.strip_suffix(" end"))
        .ok_or_else(|| format!("expected `function Name(args) end`, found `{line}`"))?;
    let (name, args) = rest
        .split_once('(')
        .and_then(|(n, a)| Some((n, a.strip_suffix(')')?)))
        .ok_or_else(|| format!("expected `function Name(args) end`, found `{line}`"))?;
    let params = args
        .split(',')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(str::to_owned)
        .collect();
    Ok((name.to_owned(), params))
}

impl Item {
    fn render(&self, out: &mut String) {
        let _ = write!(out, "### {}\n\n```lua\n", self.name);
        for annotation in &self.annotations {
            out.push_str(annotation);
            out.push('\n');
        }
        if !self.declaration.is_empty() {
            out.push_str(&self.declaration);
            out.push('\n');
        }
        out.push_str("```\n");
        for paragraph in &self.description {
            out.push('\n');
            push_wrapped(out, paragraph);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LUA_DEFINITIONS;

    const SAMPLE: &str = "---@meta
-- Header comment.

-- Section: Types
-- The types.

---A game address.
---@alias Pointer integer

-- Section: Functions supplied by eix

---Rounds.
---
---A second paragraph.
---@param value number
---@return number
function Round(value) end
";

    #[test]
    fn reads_sections_aliases_and_functions() {
        let api = LuaApi::parse(SAMPLE).expect("the sample is valid");
        assert_eq!(api.sections.len(), 2);
        assert_eq!(api.sections[0].intro, "The types.");
        assert_eq!(api.sections[0].items[0].name, "Pointer");
        assert_eq!(api.function_names(FUNCTIONS_SECTION), ["Round"]);
        let round = &api.sections[1].items[0];
        assert_eq!(round.description, ["Rounds.", "A second paragraph."]);
        let page = api.render();
        assert!(page.contains(
            "```lua\n---@param value number\n---@return number\nfunction Round(value) end\n```"
        ));
        assert!(page.contains("```lua\n---@alias Pointer integer\n```"));
    }

    #[test]
    fn a_mismatched_annotation_is_refused() {
        let bad = SAMPLE.replace("---@param value number", "---@param amount number");
        let error = LuaApi::parse(&bad)
            .expect_err("the names differ")
            .to_string();
        assert!(error.contains("Round"), "{error}");
    }

    #[test]
    fn the_definitions_file_has_the_api() {
        assert!(LUA_DEFINITIONS.is_ascii());
        let api = LuaApi::parse(LUA_DEFINITIONS).expect("the definitions file is valid");
        assert_eq!(api.function_names(FUNCTIONS_SECTION).len(), 56);
        assert_eq!(api.function_names(CALLBACKS_SECTION).len(), 19);
    }
}
