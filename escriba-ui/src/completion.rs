use escriba_core::{BufferId, CompletionEntry, Position};

use crate::float::{FloatPanel, FloatRow};

pub const POPUP_ROWS: usize = 10;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionPopup {
    pub buffer: BufferId,
    pub start: Position,
    items: Vec<CompletionEntry>,
    visible: Vec<usize>,
    selected: usize,
    navigated: bool,
}

impl CompletionPopup {
    #[must_use]
    pub fn open(
        buffer: BufferId,
        start: Position,
        mut items: Vec<CompletionEntry>,
        prefix: &str,
    ) -> Option<Self> {
        items.sort_by(|a, b| (&a.sort, &a.label).cmp(&(&b.sort, &b.label)));
        let mut popup = Self {
            buffer,
            start,
            items,
            visible: Vec::new(),
            selected: 0,
            navigated: false,
        };
        popup.narrow(prefix).then_some(popup)
    }

    pub fn narrow(&mut self, prefix: &str) -> bool {
        let wanted = prefix.to_lowercase();
        self.visible = self
            .items
            .iter()
            .enumerate()
            .filter(|(_, item)| item.filter.to_lowercase().starts_with(&wanted))
            .map(|(i, _)| i)
            .collect();
        self.selected = self.selected.min(self.visible.len().saturating_sub(1));
        !self.visible.is_empty()
    }

    pub const fn next(&mut self) {
        if !self.visible.is_empty() {
            self.selected = (self.selected + 1) % self.visible.len();
            self.navigated = true;
        }
    }

    pub const fn previous(&mut self) {
        if !self.visible.is_empty() {
            self.selected = (self.selected + self.visible.len() - 1) % self.visible.len();
            self.navigated = true;
        }
    }

    #[must_use]
    pub const fn navigated(&self) -> bool {
        self.navigated
    }

    #[must_use]
    pub fn chosen(&self) -> Option<&CompletionEntry> {
        self.items.get(*self.visible.get(self.selected)?)
    }

    #[must_use]
    pub fn labels(&self) -> Vec<&str> {
        self.visible
            .iter()
            .map(|i| self.items[*i].label.as_str())
            .collect()
    }

    #[must_use]
    pub fn entries(&self) -> Vec<&CompletionEntry> {
        self.visible.iter().map(|i| &self.items[*i]).collect()
    }

    #[must_use]
    pub fn panel(&self) -> FloatPanel {
        let first = self.selected.saturating_sub(POPUP_ROWS - 1);
        let shown: Vec<(usize, &CompletionEntry)> = self
            .visible
            .iter()
            .enumerate()
            .skip(first)
            .take(POPUP_ROWS)
            .map(|(row, i)| (row, &self.items[*i]))
            .collect();
        let label_width = shown
            .iter()
            .map(|(_, e)| e.label.chars().count())
            .max()
            .unwrap_or(0);
        FloatPanel {
            rows: shown
                .into_iter()
                .map(|(row, entry)| {
                    let mut text = String::with_capacity(label_width + 16);
                    text.push(' ');
                    text.push_str(&entry.label);
                    if let Some(note) = entry.tier.as_ref().or(entry.detail.as_ref()) {
                        for _ in entry.label.chars().count()..label_width {
                            text.push(' ');
                        }
                        text.push_str("  ");
                        text.push_str(note);
                    }
                    FloatRow {
                        text,
                        selected: row == self.selected,
                    }
                })
                .collect(),
        }
    }
}

#[must_use]
pub fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

#[must_use]
pub fn word_start(line: &str, column: u32) -> u32 {
    let chars: Vec<char> = line.chars().collect();
    let mut at = (column as usize).min(chars.len());
    while at > 0 && is_word_char(chars[at - 1]) {
        at -= 1;
    }
    u32::try_from(at).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(label: &str, sort: &str) -> CompletionEntry {
        CompletionEntry {
            label: label.to_string(),
            detail: None,
            tier: None,
            documentation: None,
            filter: label.to_string(),
            sort: sort.to_string(),
            insert: label.to_string(),
            edit: None,
            additional: Vec::new(),
        }
    }

    fn popup() -> CompletionPopup {
        CompletionPopup::open(
            BufferId(1),
            Position::new(0, 0),
            vec![entry("zip", "2"), entry("zap", "1"), entry("first", "0")],
            "",
        )
        .expect("rows")
    }

    #[test]
    fn rows_follow_the_servers_sort_order() {
        assert_eq!(popup().labels(), vec!["first", "zap", "zip"]);
    }

    #[test]
    fn narrowing_keeps_matches_and_an_empty_result_says_so() {
        let mut p = popup();
        assert!(p.narrow("z"));
        assert_eq!(p.labels(), vec!["zap", "zip"]);
        assert!(!p.narrow("q"));
    }

    #[test]
    fn opening_with_no_match_opens_nothing() {
        assert!(
            CompletionPopup::open(BufferId(1), Position::new(0, 0), vec![entry("a", "a")], "b")
                .is_none()
        );
    }

    #[test]
    fn navigation_wraps_and_marks_the_selection_as_chosen_by_the_person() {
        let mut p = popup();
        assert!(!p.navigated());
        p.previous();
        assert!(p.navigated());
        assert_eq!(p.chosen().map(|e| e.label.as_str()), Some("zip"));
        p.next();
        assert_eq!(p.chosen().map(|e| e.label.as_str()), Some("first"));
    }

    #[test]
    fn the_panel_scrolls_to_keep_the_selection_on_screen() {
        let items: Vec<CompletionEntry> = (0..30)
            .map(|i| entry(&format!("n{i:02}"), &format!("{i:02}")))
            .collect();
        let mut p =
            CompletionPopup::open(BufferId(1), Position::new(0, 0), items, "").expect("rows");
        for _ in 0..15 {
            p.next();
        }
        let panel = p.panel();
        assert_eq!(panel.rows.len(), POPUP_ROWS);
        assert!(
            panel
                .rows
                .iter()
                .any(|r| r.selected && r.text.contains("n15"))
        );
    }

    #[test]
    fn the_tier_shows_beside_the_label() {
        let mut e = entry("zip", "1");
        e.tier = Some("needs a use".to_string());
        let p = CompletionPopup::open(BufferId(1), Position::new(0, 0), vec![e], "").expect("rows");
        assert_eq!(p.panel().rows[0].text, " zip  needs a use");
    }

    #[test]
    fn a_word_starts_after_any_non_word_char_and_counts_chars() {
        assert_eq!(word_start("retsu::fi", 9), 7);
        assert_eq!(word_start("retsu::", 7), 7);
        assert_eq!(word_start("é_ab", 4), 0);
        assert_eq!(word_start("", 0), 0);
    }
}
