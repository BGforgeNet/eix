//! The game-script reference: the model of `docs/scripting.yml` and the page made from it.

use std::collections::HashSet;
use std::fmt::Write as _;

use serde::Deserialize;

use crate::{DocsError, anchor, generated_note, push_wrapped};

/// The data file's name, as the page's note names it.
const DATA_FILE: &str = "docs/scripting.yml";

/// The type of an argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Type {
    /// `f` in a signature string.
    Number,
    /// `s`.
    String,
    /// `o`.
    Object,
    /// `g`.
    Group,
}

impl Type {
    /// The type's word on the page, a TypeScript type name.
    fn word(self) -> &'static str {
        match self {
            Self::Number => "number",
            Self::String => "string",
            Self::Object => "object",
            Self::Group => "group",
        }
    }

    /// The letter of the game's signature strings, lowercase.
    fn letter(self) -> char {
        match self {
            Self::Number => 'f',
            Self::String => 's',
            Self::Object => 'o',
            Self::Group => 'g',
        }
    }
}

/// What an entry returns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Returns {
    /// A number.
    Number,
    /// A string.
    String,
    /// An object.
    Object,
    /// A group.
    Group,
    /// Nothing: a command.
    Void,
}

impl Returns {
    fn word(self) -> &'static str {
        match self {
            Self::Number => "number",
            Self::String => "string",
            Self::Object => "object",
            Self::Group => "group",
            Self::Void => "void",
        }
    }

    /// The result letter of the game's signature strings, uppercase; none for a command.
    fn letter(self) -> Option<char> {
        match self {
            Self::Number => Some('F'),
            Self::String => Some('S'),
            Self::Object => Some('O'),
            Self::Group => Some('G'),
            Self::Void => None,
        }
    }
}

/// Function or command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// Returns a value.
    Function,
    /// Returns nothing.
    Command,
}

/// Where an entry comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    /// The original game.
    Game,
    /// Added by addon.dll.
    Addon,
    /// A change made by a SpellAddon option.
    Spelladdon,
}

/// One argument.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Param {
    /// The argument's name.
    pub name: String,
    /// Its type.
    #[serde(rename = "type")]
    pub ty: Type,
    /// A call may leave it out.
    #[serde(default)]
    pub optional: bool,
    /// A call may give it again and again.
    #[serde(default)]
    pub repeat: bool,
    /// The loop variable a group function sets: an object, written in capitals in the game's signature strings.
    #[serde(default)]
    pub variable: bool,
}

/// One function or command.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    /// The name as scripts write it.
    pub name: String,
    /// Function or command.
    pub kind: Kind,
    /// The result type.
    pub returns: Returns,
    /// The arguments in order.
    #[serde(default)]
    pub params: Vec<Param>,
    /// The id of the entry's category.
    pub category: String,
    /// Where the entry comes from.
    pub source: Source,
    /// The SpellAddon option that makes the change, for a `spelladdon` entry.
    #[serde(default)]
    pub option: Option<String>,
    /// What the entry does.
    pub description: String,
    /// How eix differs from the original, where it does.
    #[serde(default)]
    pub note: Option<String>,
}

impl Entry {
    /// The signature string in the form the game uses: the result letter first for a function (`F` number, `S`
    /// string, `O` object, `G` group), then one letter per argument (`f`, `s`, `o`, `g`; `O` for a loop variable).
    /// The addon's records add `?` before an optional argument and `+` after a repeated one.
    pub fn signature(&self) -> String {
        let mut text = String::new();
        text.extend(self.returns.letter());
        for p in &self.params {
            if p.optional {
                text.push('?');
            }
            text.push(if p.variable { 'O' } else { p.ty.letter() });
            if p.repeat {
                text.push('+');
            }
        }
        text
    }

    /// The declaration as a TypeScript signature: `function GetUnitExp(unit: object, what?: string): number`.
    pub fn declaration(&self) -> String {
        let params: Vec<String> = self
            .params
            .iter()
            .map(|p| {
                if p.repeat {
                    format!("...{}: {}[]", p.name, p.ty.word())
                } else if p.optional {
                    format!("{}?: {}", p.name, p.ty.word())
                } else {
                    format!("{}: {}", p.name, p.ty.word())
                }
            })
            .collect();
        format!(
            "function {}({}): {}",
            self.name,
            params.join(", "),
            self.returns.word()
        )
    }
}

/// A section of the page.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Category {
    /// The id entries name.
    pub id: String,
    /// The section's heading.
    pub title: String,
    /// One line on what the section holds.
    pub summary: String,
}

/// The data file.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scripting {
    /// The sections, in page order.
    pub categories: Vec<Category>,
    /// The entries, in page order within a section.
    pub entries: Vec<Entry>,
}

impl Scripting {
    /// Reads and checks the data file.
    pub fn parse(text: &str) -> Result<Self, DocsError> {
        let data: Self = serde_saphyr::from_str(text)
            .map_err(|e| DocsError::new(format!("{DATA_FILE}: {e}")))?;
        data.check()?;
        Ok(data)
    }

    /// The rules the types cannot state.
    fn check(&self) -> Result<(), DocsError> {
        let fail = |what: String| Err(DocsError::new(format!("{DATA_FILE}: {what}")));
        let mut ids = HashSet::new();
        for c in &self.categories {
            if !ids.insert(c.id.as_str()) {
                return fail(format!("category {} is listed twice", c.id));
            }
        }
        let mut seen = HashSet::new();
        for e in &self.entries {
            let who = format!("entry {} ({:?})", e.name, e.source);
            if !ids.contains(e.category.as_str()) {
                return fail(format!("{who} names the unknown category {}", e.category));
            }
            if !seen.insert((e.name.to_ascii_lowercase(), e.source, e.option.clone())) {
                return fail(format!("{who} is listed twice"));
            }
            if (e.kind == Kind::Command) != (e.returns == Returns::Void) {
                return fail(format!(
                    "{who}: a command returns void, a function does not"
                ));
            }
            if (e.source == Source::Spelladdon) != e.option.is_some() {
                return fail(format!("{who}: only a spelladdon entry names an option"));
            }
            if e.params.iter().any(|p| p.variable && p.ty != Type::Object) {
                return fail(format!("{who}: a loop variable is an object"));
            }
            if let Some(at) = e.params.iter().position(|p| p.repeat)
                && at + 1 != e.params.len()
            {
                return fail(format!("{who}: only the last argument repeats"));
            }
        }
        Ok(())
    }

    /// The page, `docs/scripting.md`.
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str("# Script reference\n\n");
        out.push_str(&generated_note(DATA_FILE));
        out.push_str("\n\n");
        let intro = "The functions and commands of the game's script language. Each signature is written as a \
            TypeScript declaration. A function returns a value; a command returns `void`. The types are `number`, \
            `string`, `object` and `group`; `name?` is an argument a call may leave out, and `...name` may be given \
            again and again. Names ignore ASCII case. A condition is a number: 0 is false.";
        push_wrapped(&mut out, intro);
        out.push('\n');
        let tags = "Entries tagged <span class=\"eix\">eix</span> are added or changed by eix: the functions and \
            commands that addon.dll adds, and the changes that SpellAddon options make to the game's own. The others \
            are the original game's. A note headed *Differs from the original* says where eix behaves differently.";
        push_wrapped(&mut out, tags);
        out.push_str("\nSections:\n\n");
        for c in &self.categories {
            let _ = writeln!(out, "- [{}](#{})", c.title, anchor(&c.title));
        }
        for c in &self.categories {
            let _ = write!(out, "\n## {}\n\n", c.title);
            push_wrapped(&mut out, &c.summary);
            for e in self.entries.iter().filter(|e| e.category == c.id) {
                out.push('\n');
                render_entry(&mut out, e);
            }
        }
        out
    }
}

fn render_entry(out: &mut String, e: &Entry) {
    match &e.option {
        Some(option) => {
            let _ = write!(out, "### {} - SpellAddon `{}`\n\n", e.name, bare(option));
        }
        None => {
            let _ = write!(out, "### {}\n\n", e.name);
        }
    }
    let _ = write!(out, "```ts\n{}\n```\n\n", e.declaration());
    match (e.source, &e.option) {
        (Source::Addon, _) => {
            out.push_str("<span class=\"eix\">eix</span> Added by addon.dll.\n\n");
        }
        (Source::Spelladdon, Some(option)) => {
            let _ = write!(
                out,
                "<span class=\"eix\">eix</span> SpellAddon option `{option}`.\n\n"
            );
        }
        _ => {}
    }
    push_wrapped(out, &e.description);
    if let Some(note) = &e.note {
        out.push('\n');
        push_wrapped(out, &format!("*Differs from the original:* {note}"));
    }
}

/// An option's key without its section: `[speed] gsgetvar` is `gsgetvar`.
fn bare(option: &str) -> &str {
    option.rsplit_once(' ').map_or(option, |(_, key)| key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SCRIPTING_DATA;

    const SAMPLE: &str = "
categories:
  - id: c
    title: Things
    summary: >-
      Some things.
entries:
  - name: GetUnitExp
    kind: function
    returns: number
    params:
      - {name: unit, type: object}
      - {name: what, type: string, optional: true}
    category: c
    source: addon
    description: >-
      Returns experience.
    note: >-
      Same.
  - name: Say
    kind: command
    returns: void
    params:
      - {name: text, type: string, repeat: true}
    category: c
    source: spelladdon
    option: \"[gameplay] saytrans\"
    description: >-
      Says it.
";

    #[test]
    fn declarations_and_signatures() {
        let data = Scripting::parse(SAMPLE).expect("the sample is valid");
        assert_eq!(
            data.entries[0].declaration(),
            "function GetUnitExp(unit: object, what?: string): number"
        );
        assert_eq!(data.entries[0].signature(), "Fo?s");
        assert_eq!(
            data.entries[1].declaration(),
            "function Say(...text: string[]): void"
        );
        assert_eq!(data.entries[1].signature(), "s+");
    }

    #[test]
    fn the_page_tags_and_notes_entries() {
        let page = Scripting::parse(SAMPLE)
            .expect("the sample is valid")
            .render();
        assert!(page.contains("### GetUnitExp\n"));
        assert!(page.contains("### Say - SpellAddon `saytrans`\n"));
        assert!(page.contains("<span class=\"eix\">eix</span> Added by addon.dll."));
        assert!(page.contains("SpellAddon option `[gameplay] saytrans`."));
        assert!(page.contains("*Differs from the original:* Same."));
    }

    #[test]
    fn bad_data_is_refused() {
        let unknown = SAMPLE.replace(
            "category: c\n    source: addon",
            "category: x\n    source: addon",
        );
        assert!(Scripting::parse(&unknown).is_err());
        let void = SAMPLE.replace("returns: void", "returns: number");
        assert!(Scripting::parse(&void).is_err());
        let option = SAMPLE.replace("    option: \"[gameplay] saytrans\"\n", "");
        assert!(Scripting::parse(&option).is_err());
        let field = SAMPLE.replace("kind: command", "kind: command\n    colour: red");
        assert!(Scripting::parse(&field).is_err());
    }

    #[test]
    fn the_data_file_is_valid_and_ascii() {
        assert!(SCRIPTING_DATA.is_ascii());
        let data = Scripting::parse(SCRIPTING_DATA).expect("the data file is valid");
        assert!(data.entries.len() > 250);
    }
}
