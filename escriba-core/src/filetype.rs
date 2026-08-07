//! Filetypes and their comment syntax — the seam between what a buffer IS
//! and how the editor treats it.
//!
//! `(defmode :name "rust" :extensions ("rs") :commentstring "// %s")` has
//! parsed and validated since Wave 1, and nothing has ever consumed the
//! commentstring. This is the table that does, and it is deliberately in
//! `escriba-core` rather than beside the parser: the authoring layer
//! populates it, the dispatch seam exposes it, a command reads it, and the
//! interpreter never touches it. A type four crates share belongs to none of
//! them.

use std::path::Path;

/// A comment syntax, parsed from vim's `commentstring` form.
///
/// `"// %s"` becomes `("// ", "")`; `"<!-- %s -->"` becomes
/// `("<!-- ", " -->")`. Splitting ONCE, here, is what lets every caller
/// treat line and block comment styles identically instead of each one
/// re-discovering that `%s` can have a suffix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommentString {
    pub prefix: String,
    pub suffix: String,
}

impl CommentString {
    /// Parse a `commentstring`. `None` when there is no `%s` placeholder.
    ///
    /// The rejection matters: a commentstring without `%s` cannot say where
    /// the content goes, so a caller given one would have to guess. Refusing
    /// at parse time means no downstream code contains a guess. Honest tier:
    /// **parse-time-rejected** — the type cannot be built wrong, though a
    /// `defmode` carrying a bad string still parses as Lisp.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        let (prefix, suffix) = s.split_once("%s")?;
        Some(Self {
            prefix: prefix.to_string(),
            suffix: suffix.to_string(),
        })
    }

    /// Wrap `content` in this comment syntax.
    #[must_use]
    pub fn wrap(&self, content: &str) -> String {
        let mut out = String::with_capacity(self.prefix.len() + content.len() + self.suffix.len());
        out.push_str(&self.prefix);
        out.push_str(content);
        out.push_str(&self.suffix);
        out
    }

    /// Strip this comment syntax from `line`, if it is commented.
    ///
    /// Tolerant on purpose: `//x`, `// x` and `//  x` are all commented, and
    /// an editor that only recognised its own exact output would refuse to
    /// uncomment anything a human typed. So the marker is matched WITHOUT
    /// its conventional trailing space.
    ///
    /// Exactly ONE separating space is then removed, which is what makes
    /// wrap→strip a round trip while leaving deliberate alignment alone:
    /// `//  indented` uncomments to ` indented`, not `indented`.
    #[must_use]
    pub fn strip<'a>(&self, line: &'a str) -> Option<&'a str> {
        let body = line.trim_start_matches([' ', '\t']);
        let body = body.strip_prefix(self.prefix.trim_end())?;
        let body = body.strip_prefix(' ').unwrap_or(body);
        let suffix = self.suffix.trim_start();
        let body = if suffix.is_empty() {
            body
        } else {
            let cut = body.strip_suffix(suffix).unwrap_or(body);
            cut.strip_suffix(' ').unwrap_or(cut)
        };
        Some(body)
    }

    /// Is `line` already commented in this syntax?
    #[must_use]
    pub fn is_commented(&self, line: &str) -> bool {
        self.strip(line).is_some()
    }
}

/// One filetype's editor-facing facts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Filetype {
    /// The mode name — `"rust"`, `"lisp"`.
    pub name: String,
    /// Extensions that select it, without a leading dot.
    pub extensions: Vec<String>,
    /// Comment syntax, if the mode declared a usable one.
    pub comment: Option<CommentString>,
}

/// Extension → filetype.
///
/// A plain scan rather than a `HashMap`: a real editor knows tens of
/// filetypes, the lookup happens once per command, and a map would be a
/// second structure to keep in step with the authored list for no measurable
/// gain. (The same reasoning as `BufferSet::find_by_path` — derive, do not
/// cache, when the set is small.)
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FiletypeTable {
    entries: Vec<Filetype>,
}

impl FiletypeTable {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a filetype. A later entry for the same extension wins, matching
    /// the last-writer-wins semantics every other apply path uses.
    pub fn insert(&mut self, ft: Filetype) {
        self.entries.retain(|e| e.name != ft.name);
        self.entries.push(ft);
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The filetype for `path`, by extension.
    #[must_use]
    pub fn resolve(&self, path: &Path) -> Option<&Filetype> {
        let ext = path.extension()?.to_str()?;
        // Later entries win, so scan backwards.
        self.entries
            .iter()
            .rev()
            .find(|f| f.extensions.iter().any(|e| e == ext))
    }

    #[must_use]
    pub fn by_name(&self, name: &str) -> Option<&Filetype> {
        self.entries.iter().find(|f| f.name == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_commentstring_without_a_placeholder_is_refused() {
        // It cannot say where the content goes, so every caller would have
        // to guess. Refusing here means no caller contains a guess.
        assert_eq!(CommentString::parse("//"), None);
        assert_eq!(CommentString::parse(""), None);
        assert!(CommentString::parse("// %s").is_some());
    }

    #[test]
    fn line_and_block_styles_parse_the_same_way() {
        let line = CommentString::parse("// %s").expect("line style");
        assert_eq!(line.prefix, "// ");
        assert_eq!(line.suffix, "");

        let block = CommentString::parse("<!-- %s -->").expect("block style");
        assert_eq!(block.prefix, "<!-- ");
        assert_eq!(block.suffix, " -->");
    }

    #[test]
    fn wrapping_then_stripping_is_a_round_trip() {
        for cs in ["// %s", "<!-- %s -->", ";; %s", "# %s"] {
            let c = CommentString::parse(cs).expect(cs);
            let wrapped = c.wrap("hello");
            assert_eq!(c.strip(&wrapped), Some("hello"), "{cs}");
            assert!(c.is_commented(&wrapped), "{cs}");
        }
    }

    #[test]
    fn stripping_tolerates_what_a_human_would_type() {
        // An editor that only recognised its own exact output would refuse
        // to uncomment anything anyone else wrote.
        let c = CommentString::parse("// %s").expect("parses");
        assert_eq!(c.strip("//x"), Some("x"), "no space");
        assert_eq!(c.strip("// x"), Some("x"), "one space");
        assert_eq!(c.strip("    // x"), Some("x"), "indented");
        assert_eq!(
            c.strip("//  aligned"),
            Some(" aligned"),
            "exactly ONE space is the separator; the rest is deliberate",
        );
        assert!(!c.is_commented("let x = 1;"), "plain code is not a comment");
        assert!(!c.is_commented(""), "an empty line is not a comment");
    }

    #[test]
    fn resolution_is_by_extension_and_misses_are_none() {
        let mut t = FiletypeTable::new();
        t.insert(Filetype {
            name: "rust".into(),
            extensions: vec!["rs".into()],
            comment: CommentString::parse("// %s"),
        });
        assert_eq!(
            t.resolve(Path::new("src/main.rs")).map(|f| f.name.as_str()),
            Some("rust"),
        );
        assert!(t.resolve(Path::new("notes.txt")).is_none());
        // A path with no extension at all must not panic.
        assert!(t.resolve(Path::new("Makefile")).is_none());
    }

    #[test]
    fn a_later_entry_replaces_an_earlier_one_of_the_same_name() {
        // Last-writer-wins, matching every other apply path: a user rc
        // redefining `rust` must not leave two `rust` entries racing.
        let mut t = FiletypeTable::new();
        t.insert(Filetype {
            name: "rust".into(),
            extensions: vec!["rs".into()],
            comment: CommentString::parse("// %s"),
        });
        t.insert(Filetype {
            name: "rust".into(),
            extensions: vec!["rs".into()],
            comment: CommentString::parse("//! %s"),
        });
        assert_eq!(t.len(), 1);
        assert_eq!(
            t.resolve(Path::new("a.rs"))
                .and_then(|f| f.comment.as_ref())
                .map(|c| c.prefix.as_str()),
            Some("//! "),
        );
    }
}
