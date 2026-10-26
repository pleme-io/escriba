# Insert mode erase keys: the bigger erases (2026-08-09)

**The bigger erases followed the same afternoon.** `<BS>`/`<Del>` landed
first and `<C-w>`/`<C-u>`/`<C-h>` were left behind, so Insert mode could
erase one character at a time and nothing larger — a mistyped word had to
be dismantled letter by letter. `Action::PromptDeleteWord`/
`PromptClearToStart` are now `DeleteWordBefore`/`DeleteToLineStart`, same
one-action-three-targets shape (`:action "delete-word-before"` /
`"delete-to-line-start"`), and all three backward erases share ONE
`erase_back_to(target)` body so the no-register-capture and
viewport-follow properties are stated once rather than three times.
`<C-w>` reaches back over `Motion::WordStartPrev` — the SAME resolver the
cursor move and the operator range stand on — so `<C-w>` and `db` agree on
where a word starts by construction. Two shape details: `word_prev` is
single-line and returns the cursor unchanged at column 0, so `<C-w>` falls
through to `delete_before_cursor` there (vim erases the line break);
`<C-u>` is two-step, stopping at the first non-blank before taking the
indent, because collapsing it to "always column 0" destroys alignment on
the press the hands actually reach for.
