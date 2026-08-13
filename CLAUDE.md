# escriba

## Start screen — `(defsplash …)` (2026-08-07, shipped)

`escriba` with no file argument opens on a start screen: the wordmark,
a tagline, a five-entry menu, and a footer strip. `escriba <file>` never
shows it — a welcome screen in front of a file you asked for is a
keystroke tax.

**One model, three faces.** `escriba_ui::splash::Splash` owns ALL the
layout (centering, block grouping, degradation); the faces only color it.
Two projections, both derived from `rows()` so they cannot disagree:
`rows()` for a face that positions widgets (ratatui), `screen_chunks()`
for the two that emit a character stream (the ANSI dump, and the GPU's
`set_rich_text`, which needs a coverage-complete partition). Colors are
ROLES resolved through `ChromePalette` — the same seam the rest of the
chrome uses — so the screen tracks a theme change for free.

**Authored, not hardcoded.** There is deliberately no
`Splash::default()`. The content is `(defsplash …)` in
`configs/blnvim-defaults.lisp`, lowered by
`escriba_lisp::apply_plan_to_splash`. Entry `:action` strings go through
`escriba_lisp::resolve_action` — the SAME table `defkeybind` uses (made
`pub` for this), so a menu entry and a key bound to the same string
dispatch identically. The shipped menu lists only actions wired TODAY
(`normal` / `insert` / `search-forward` / `command` / `quit`); a pressed
key that did nothing would be worse than a shorter menu.

**Two traps worth remembering:**

- **`:disabled #t`, not `:enable #f`.** A key absent from a tatara-lisp
  form does NOT reach serde's `#[serde(default = …)]` — it takes the
  field type's zero value. An `:enable: bool` therefore made a bare
  `(defsplash …)` parse cleanly and never appear. Verified, not assumed:
  the first cut shipped with `enable = false`. Polarity is inverted here
  relative to `defeffect` on purpose, to put the safe state on the zero
  value.
- **The screen owns the FIRST keypress only.** A menu key runs its
  entry; any other key dismisses and is then handled normally, so the
  first thing you type is never eaten. It is `Option<Splash>` on
  `EditorState`, deliberately NOT a `Mode` variant — a mode is a state
  keys are interpreted *in*, and this interprets exactly one key.

`--no-splash` / `$ESCRIBA_NO_SPLASH` skip it for one run; `--list-rc`
reports it in the wiring-status block. Footer facts are computed from the
plan the editor actually booted with, and the theme name comes from
`chrome::prescribed_theme_name()` (what is PAINTED) rather than
`plan.theme` (what was DECLARED) — see the theming gap below.

## Search reports as SEARCH, not COMMAND (2026-08-07, fixed)

vim's `/` is the command line, so escriba's search genuinely runs in
`Mode::Command`. The status line reported the raw mode, which meant
`/foo` rendered `: COMMAND` — character-for-character the line `:`
produces — with the pattern parked at the far RIGHT, past the match
count. Search was fully working and read as "pressing `/` put me in `:`
mode". **The model was right and the report was wrong.**

Fixed at the shared model so both faces inherit it:
`StatusModel::mode_label()` says `SEARCH` when a search prompt is open,
and `pill_sigil()` gives the pill `/` or `?` instead of `:`. Both derive
from the same typed `PromptKind` as the sigil, so label and sigil cannot
disagree. The TUI also moved the prompt to the LEFT, where vim puts the
cmdline. Pinned by `escriba-tui/tests/status_line_frame.rs`, which
asserts on rendered CELLS — the unit tests all passed throughout the
period this was broken.

`resolve_action` also gained `search-forward` / `search-backward` /
`search-next` / `search-prev`, which were missing: `:action
"search-forward"` silently became a lookup for a command that does not
exist.

## Test coverage of the GPU face — what is and isn't claimed

`GpuRenderer::render` needs a live `madori::RenderContext` (wgpu device +
surface view), so it cannot run under `cargo test`. Rather than leave the
whole face uncovered, the parts that can actually be WRONG were extracted
into pure functions and tested in `escriba-render/tests/gpu_logic.rs`:

- **`cell_grid(w_px, h_px, font_size, line_height)`** — the pixel→character
  conversion. This was duplicated: `resize` divided by line-height then
  subtracted a row, `render` subtracted a line-height then divided. Same
  intent, two spellings, two chances to reserve the status row wrong. Now
  one function, covering degenerate surfaces (0×0 on minimise), degenerate
  font metrics (`with_font_size` is public and unvalidated), and
  monotonicity under resize.
- **`splash_runs(chunks, palette)`** — the role→colour mapping. A mis-mapped
  role paints menu keys as body text and renders perfectly; glyphon cannot
  notice, so this is where the check has to live. Public *so the test calls
  the real function* — a test that rebuilt the mapping would keep passing
  after the renderer stopped calling it.

**The honest claim is "the GPU face's LOGIC is tested", not "the GPU face is
tested".** What remains uncovered is glyphon/wgpu call sequencing — buffer
sizing, shaping, encoder submission — which is upstream's contract. If you
add branching logic to `render()`, extract it rather than growing the
untestable region.

## Theming — how escriba paints (2026-07-26)

**One seam: `escriba_ui::chrome::ChromePalette`.** All THREE faces (the
ratatui TUI chrome, the GPU backend, and the ANSI text dump) resolve every
color through it, and it resolves through `ishou_tokens::SemanticRoles` — the
closed ROLE vocabulary — never through a theme's own token spelling. *(The
text face was corrected 2026-08-07: it still read `VellumPalette::vellum()`
directly, missed because this note said "both faces" when there are three.)* Consequences worth knowing:

- **Colors are named by role, not hue.** `c.info`, not "cyan". On Nord
  `info` is frost blue; on Vellum it was ice cyan; no call site knows or
  cares. This is what makes a theme change a value change instead of a
  fleet-wide rename.
- **`ChromePalette::for_theme` is total over `FleetTheme`** — no wildcard
  arm, so adding a theme upstream fails `chrome.rs` to compile rather than
  silently painting the wrong thing.
- **There are no hex literals in the paint path.** The last one
  (`Color::Rgb(0xCD, 0xC7, 0xB6)`, "statusline_fg") is gone.
- **Both convergence guards assert `FleetTheme::prescribed_default()`**, not
  a hand-written constant, so they cannot be satisfied by a stale literal.

**Why this landed:** the renderers were hardwired to
`VellumPalette::vellum()`. When the fleet moved its prescribed theme from
Vellum to PlemeDark (Nord — what mado ships), escriba kept painting Vellum
and both guards went RED (`theme drift — actual Vellum != fleet PlemeDark`).
`(deftheme :preset …)` had the same defect from the other side: it resolved
to a real `FleetTheme` that **nothing outside tests consumed**.

**Closed 2026-08-07 — `(deftheme :preset …)` now reaches the pixels.**
The paint path used to read `ChromePalette::prescribed()` at every site, so
an operator could author `(deftheme :preset "vellum")`, watch `--list-rc`
report it, and see Nord on screen. The fix, in three parts:

- **`EditorState` owns the theme** (`theme` + resolved `chrome`), so there
  is ONE answer to "what colour is this editor" and every face reads it.
  `set_theme` bumps the refresh generation — without that the GPU face
  keeps its cached shaped buffer and the old colours stay up until an
  unrelated edit invalidates it.
- **Every paint site takes the palette as an ARGUMENT.** The TUI style
  helpers and the GPU's `ground_bg` / `mode_color` no longer reach for the
  default themselves. A palette that arrives as a parameter cannot be
  ignored; one fetched inside the function always could be.
- **The binary applies `plan.theme.resolve()`** before anything paints.

`clear_frame` still paints the prescribed ground, correctly: it runs when
state could not be read, which is exactly when the operator's theme is
unknowable.

**What wiring it surfaced:** the shipped rc declared `(deftheme :preset
"vellum")` — stale since the fleet moved to Nord, and invisible for as long
as nothing consumed it. It now says `nord`, and
`theme_declaration_agrees_with_the_fleet_default` asserts the declaration
against `FleetTheme::prescribed_default()` rather than against a literal,
so a fleet re-point fails in the one file that decides it. Vellum remains
opt-in via `--rc escriba/configs/vellum.lisp`.

skip-fleet-convergence-guard: EscribaConfig (escriba-config/src/lib.rs)
exposes NO `font_family` / `font_size` / `cursor` fields that participate
in `ishou_tokens::FleetDefaults::prescribed()` — those visual primitives
live in escriba-render's GPU target, not in EscribaConfig — so the
ishou_tokens convergence Guard has nothing to assert against. (Since
2026-06-26, `prescribed_default()` DOES pin scalar defaults — `tema`
"vellum" + line numbers + tab-width 2 + statusline — mirroring the
load-bearing `configs/blnvim-defaults.lisp` so `config-show default`
is honest; but none of those are FleetDefaults font/cursor primitives,
so the waiver stands.) Reconsider if a future EscribaConfig revision
introduces typed `font_family` / `font_size` / `cursor` fields.

## Default config (the shipped blnvim-parity baseline)

`nix run .#escriba` boots a baked-in curated default —
`escriba/configs/blnvim-defaults.lisp` (`include_str!` at
`escriba/src/lib.rs:40`) + the 45-entry bundled plugin catalog
(`catalog_bundle.rs`) — applied at boot unless `--no-defaults`. The
default mirrors blackmatter-nvim (blnvim): leader `,` (blnvim parity),
tab-width 2, ~19 `:set` options, `<C-s>` save, 14 tree-sitter major
modes, the fleet-prescribed theme (Nord), ~30 highlights, and the start
screen. **Tier-honest parity gap:** the WIRED def-form set is
`defcmd`/`defoption`/`defkeybind`/`defmode`/`deftheme`/`defsplash` — so
the ~40 catalog plugins that declare picker/LSP/git/completion/formatter/
diagnostic keybinds, and the operator-edit verbs (`dw`/`ciw`/paste), are
**bound-but-inert** until their subsystem waves land (a running LSP
client, a picker, a git layer). Closing those = escriba feature-work, not
a config change.

**That inert set is no longer a prose claim.** `escriba/tests/action_resolution.rs`
pins the exact shipped keybind actions that resolve to neither a typed
`Action` nor a registered command, and asserts SET EQUALITY — so a typo
fails (it is not in the list) and wiring a subsystem also fails until its
names are removed. The `:action` fallback to `Action::Command` is
load-bearing (it is how a plugin command resolves later) and is exactly
what makes a typo and a not-yet-wired subsystem indistinguishable at
parse, apply and dispatch time; the list is what tells them apart. Start
screen entries are held to the stricter bar of zero unresolvable, because
an inert keybind is invisible until pressed while an inert menu entry is
printed as an offer.

**Operator-over-motion engine (2026-06-26, shipped):** the vim
`{operator}{motion}` verbs (`dw`/`c$`/`y0`) execute in `escriba-runtime`
— `Action::ApplyOperator { op, motion }` is no longer a no-op.
Composition stands on a pure `resolve_motion(from, motion) -> Position`
(extracted so the cursor-move path AND the operator-range path share one
motion-resolution source of truth); Delete/Change/Yank act over the
resolved `[cursor, target)` range with undo, Change enters Insert, a
single unnamed `register` captures deleted/yanked text. Indent/Format/
structural operators are named-but-unwired. The keymap **operator-pending
FSM** is now wired: `d`/`c`/`y` are bound to `Action::Operator(_)` and a
small `(State,Event)->(State,effects)` machine (`operator_pending.rs`)
holds the pending operator until the next motion composes
`Action::ApplyOperator`. That FSM **stands on the fleet `zenmai`
primitive** (escriba is zenmai's 3rd cross-repo consumer, after bolso +
gaveta) — the editor doesn't re-roll a bespoke `Option<Operator>` + dispatch
`if let`s. So typing `d` then `w` deletes a word from the keyboard today.
**Counts compose correctly** (`3dw` = 3 words, `2d3l` = 6 chars): the FSM's
event/effect is `(Action, count)` and it owns count composition (operator
count × motion count), so a bare `5j` still passes through unchanged — the
prior naive outer repeat-loop, which silently broke `3dw`, is gone.
**Landed 2026-08-08 — `dd`, text objects, and counted operators.**
`dd`/`cc`/`yy` are linewise (a doubled operator composes
`TextObject::Line`, and a DIFFERENT operator re-arms rather than
cancelling, because `dc` is a typo for `cc`). Text objects are real —
`iw`/`aw` on vim's three character classes, and `i(`/`a{`/`i"`… over a
depth-counting bracket scan, with `open == close` telling the resolver
not to count nesting for quotes.

**Object selection is a KEY-layer concern, and that is load-bearing.**
`i` is `ChangeMode(Insert)` in Normal and `a` and every bracket are
UNBOUND, so all of them reach the operator FSM as `Action::Pending` with
the character already discarded — `di(` is undecidable from actions. One
`Option<bool>` read before sequence resolution (`consume_object_key`)
does what vim's operator-pending keymap does. It must run before
everything, or `di(` reads as `d` → `i` (insert) → literal `(`.

**A counted operator is ONE operation over an N-resolved motion**, not N
operations over one motion. `3dw` is one delete over three-words-forward.
The distinction is invisible for delete — removing text shifts what is
under the cursor, so repetition works by accident — and wrong for yank,
which does not move the cursor and so re-yanked the same word. This is
why the "combined register for counted deletes" this line used to ask for
does not exist: with one operation there is only ever one yank.

**Still remaining:** the named-but-unwired operators (Indent / Format /
structural), named registers (`"ay`), and objects that span lines (the
bracket scan is single-line today).

## `:wq` was not a command name — it was a spelling nobody parsed (2026-08-09, fixed)

`:w` saved and `:q` quit, and `:wq` said "command not found". The ex line was
resolved by a three-arm `match` at the bottom of `escriba-runtime`
(`"w" => "save"`, `"q" => "quit"`, `"u" => "undo"`) and everything else fell
through to a registry lookup — so there was nowhere for a *compound* spelling,
an abbreviation, or a `!` to be known. **The two halves were each provably
fine and the pair was broken.**

The grammar is now a TABLE in `escriba_command::ex`: full name + minimum
prefix + the registry command for the plain and the banged form, which is vim's
`:q[uit]` notation made executable. `:w :wr :writ :write :wq :wq! :wa :wall
:wqa :wqall :x :xit :xa :xall :exi :exit :q :qu :quit :q! :qa :qall :quita
:quitall :u :red` all resolve; a word the table does not know passes through
**unchanged** so `:noh`, `:picker.files` and every plugin command still
dispatch — the grammar covers the vim vocabulary, it does not fence the
namespace.

Two properties are asserted rather than asserted-to, over the whole table
rather than a sample: **every prefix from a verb's minimum to its full
spelling resolves to it**, and **no spelling is ambiguous**. The second is a
property of the MINIMUMS and is exactly why `quitall`'s is five characters —
without it `:q` would be ambiguous and `resolve` would silently pick whichever
came first in the table.

**One registered command per distinct BEHAVIOUR, many spellings.** `plain` and
`forced` are two command names (`quit` / `quit!`), not one name plus a bang
argument: a body receives `&[String]` and nothing else, so a bang-as-argument
is a convention every body must remember to read, and the one that forgets
quits without asking.

**The bang now means something, which is a behaviour change.** `:q` on a
modified buffer declines with vim's E37 and `:q!` overrides; before, the two
were the same keystroke sequence at different lengths and the editor discarded
unsaved work silently. `:wq` on an unnamed buffer declines with E32 rather than
exiting as though it had written. `:x` writes only when modified — the mtime
difference is the whole point of it existing next to `:wq`.

**What wiring it surfaced: the TUI status line never drew `model.message`.**
E37, E486, E35, "command not found" — every refusal the editor made was silent
in the DEFAULT face, while the text face and every unit test saw it.
`action_dispatch.rs` asserts the message reaches the MODEL and stayed green
throughout. Same shape as the `: COMMAND` bug above: the model right, the
report missing. Tier-honest: the message persists until another replaces it,
because `EditorState::messages` is also the `:messages` log and there is
nothing to clear without losing the log — both other faces behave the same
way.

## `w` walked off the end of the text, and `e` was `w` under another name (2026-08-09, fixed)

Three defects, one report. All three are pinned in
`escriba-runtime/tests/word_motions.rs`.

- **The cursor rested past the last character.** `w` onto the final word
  landed one column past it. The POSITION was right — it is the exclusive end
  an operator needs, and `dw` on the last word must delete the whole word — so
  the fix is the *cursor's* rule, not the motion's: **in Normal mode the cursor
  sits ON a character.** It lives in `place_cursor`, the single cursor-mutation
  path, so `$`, `x` at end of line and every future forward motion inherit it,
  while `resolve_motion` stays pure and the operator range is untouched.
  `Buffer::clamp` could not make this call: it answers "is this position inside
  the text", which is a question about the BUFFER, and one-past-the-end
  legitimately is.
- **An edit's advance is not a rest.** The clamp took a `CursorRest` parameter
  the same afternoon it was written, because the lisp `(insert …)` effect runs
  in NORMAL mode: clamping its advance back onto the last character makes the
  next `(insert …)` land *inside* the text just written. `OnCharacter` is a
  motion's destination; `AtInsertPoint` is where the next character goes.
- **`w` split words on whitespace alone.** `object_word` had grown vim's three
  classes (word / punct / space) while the MOTIONS had not, so `diw` on
  `foo.bar` took `foo` and `dw` took `foo.bar` — two answers from one editor on
  the same text. One `word_class` now; `b` is class-aware too, or `dw` and `db`
  would disagree about where the same word begins. `w` also crosses onto the
  next line's first non-blank rather than column 0, and stops on an empty line
  the way vim does.
- **`Motion::WordEndNext` resolved through `word_next`** — `e` and `w` were the
  same function with two names. `e` is now its own resolver and is bound.
  It is also vim's first INCLUSIVE motion (`Motion::is_inclusive`), widened to
  an exclusive end at the OPERATOR and never in the resolver: `e` must land ON
  the character and `de` must delete THROUGH it. Getting that backwards leaves
  exactly one letter behind.

**The phantom trailing line is a real defect and is NOT fixed.** A file ending
in `\n` is one line plus a terminator, but the rope reports two lines and the
second gets a gutter number in every face — `--render=text` on `hello world\n`
draws a line 2. The word motions stop at `last_text_line` so they no longer
walk the cursor onto it, which is a claim about the motions ("there is no next
word after the last character") and not a fix for the row being drawn. Hiding
it is a buffer-model change with a much wider blast radius.

## The rest of the vim movement suite (2026-08-13, landed)

`h j k l w b e 0 $ G` and the operators were the whole of it. Everything
else vim moves with is now here: **`W`/`E`/`B`/`gE`** (the WORD width —
whitespace-delimited, so `foo.bar` is one WORD and three words),
**`ge`**, **`f`/`F`/`t`/`T`** and **`;`**, **`%`**, **`{`/`}`**,
**`(`/`)`**, **`H`/`M`/`L`**, **`^`/`_`/`g_`**, **`|`**, **`+`/`-`**,
**`<C-f>`/`<C-b>`/`<C-d>`**. Each is a `Motion` arm with a resolver, so
each composes with an operator and a count for free — `d}`, `3fx`, `dg_`
and `y%` all work because `apply_operator` was already the one place a
motion becomes an edit.

Four things are worth knowing before touching this:

- **`f`'s operand is a KEY, not a binding**, so `consume_find_key` claims
  it before the sequence stepper and before the keymap — exactly where
  `consume_object_key` claims `di(`. Otherwise `fw` reads as `f` then
  *move a word* and `fi` enters Insert. The consequence is that
  `f`/`F`/`t`/`T` must stay UNBOUND: a binding on one of them is a table
  entry no keypress can reach, which reads as configured and behaves as
  absent. `movement_survives_defaults.rs` asserts that absence.
- **`|` is the one motion whose count is an ARGUMENT.** `40|` means
  column 40, not column 1 forty times (which is column 1). Folded in at
  `apply_counted` before the operator FSM sees it, so the machine keeps
  its single rule — counts repeat — and the exception lives in one place.
- **`;` cannot answer `is_inclusive` by itself.** Whether it widens
  depends on the direction of the find it repeats, which is runtime
  state, so `operated_end` resolves it to the concrete `FindChar` and
  asks that. `d;` after `fx` must delete THROUGH the `x`.
- **`,` is NOT bound, and that is a decision.** escriba's shipped leader
  IS `,` (blnvim parity), and the keymap's rule is that a single binding
  wins over a sequence prefix — so binding `,` would have silently killed
  all 93 `<leader>…` bindings the catalog ships. Reverse-repeat is
  authorable as `:action "find-reverse"`; `F`/`T` search backwards
  directly.

**What wiring it surfaced — three more `<C-h>`-class shadowings.**
`<S-h>`/`<S-l>` (bufferline's buffer prev/next) ARE `H` and `L`, and `-`
(oil's parent directory) is vim's previous-line motion. All three were
bound by bundled caixas applied ON TOP of the default keymap, so all three
motions were dead in the shipped build while every unit test stayed green
— the tests build `Keymap::default_vim()`, which was correct. They moved
to `[b`/`]b` and `<leader>-`. `escriba/tests/movement_survives_defaults.rs`
is the gate: it builds the keymap the BINARY boots with and fails, naming
the caixa's own action, if any motion or operator key is displaced again.
That file and `insert_erase_survives_defaults.rs` are the only two tests
in the repo that see the composite plan; a key defect that is invisible to
everything else will be visible to exactly these.

`ff` (blnvim's bare format binding) was removed to make room for `f` —
it was a sequence left with a note saying it would "need deciding when `f`
lands". `lsp.format` keeps `<leader>lf`, `:Format` and its `BufWritePre`
hook; only the fourth spelling is gone.

Pinned in `escriba-runtime/tests/movement_suite.rs` (30 tests, all
key-driven — the fixture even walks the cursor to its start position by
keys, because a `set_cursor` hook can place it where no keystroke can and
then prove a motion from a state the editor never reaches).

### The second pass — marks, `z`-scroll, `<C-u>`, matchit (same day)

The four things the first pass named as missing are in.

- **Marks.** `m{a-z}` sets, `` `{a-z} `` returns to the exact position,
  `'{a-z}` to the line's first non-blank. Two `Motion` arms, not one plus
  a flag: `` `a `` is exclusive and `'a` is linewise, so ``d`a`` and
  `d'a` delete different things. An UNSET mark is a failed motion —
  `` `q `` leaves the cursor alone and ``d`q`` leaves the buffer alone,
  rather than jumping to the origin and deleting to the top of the file.
  Only `a-z`: `A-Z` are vim's cross-file marks and this map is
  per-editor, so accepting one would promise a jump to another FILE and
  deliver a jump to that line in this one.
- **`zt`/`zz`/`zb`** re-frame the window and do NOT move the cursor —
  which is the whole reason they are an `Action::ScrollView`, not a
  motion. Folding them into `Motion` would make them composable with an
  operator, and `dzz` is not a thing. They must also not route through
  `set_cursor`, which scrolls the viewport to contain the cursor and
  would undo the re-frame it was just asked for.
- **`<C-u>` was never conflicted.** The first pass listed it as "taken by
  the insert-mode erase". Bindings are per-mode and the erase is bound in
  Insert and Command only, so Normal was free the whole time. It is
  half-page-up now, and a test pins both readings coexisting.
- **matchit** — `%` over language WORD pairs. A typed table keyed by
  filetype name (`if`/`elif`/`else`/`fi`, `do`/`end`, `case`/`esac`, …),
  walked by the same depth-counting scan as the bracket case. Middles are
  first-class: `%` STEPS THROUGH `elif` and `else` on its way to `fi`
  rather than jumping straight past them, which is what makes it useful
  for reading a chain rather than just finding its end. Word-BOUNDED on
  both sides, so `endif` is not read as `end` and `notify` contains no
  keyword — an unbounded scan is worse than no word pairs at all, because
  a `%` that lands inside an identifier is a silent wrong answer.

**Two more defects the wiring surfaced, both invisible to every test:**

- **`gg` was never bound.** `G` was; `gg` was not, and the only `gg` in
  the repo was a unit test that *bound it itself* before pressing it — so
  it proved the sequence machinery worked and said nothing about the
  default keymap, while vim's most-pressed motion did nothing in the
  shipped editor. A test that constructs the thing it is checking cannot
  fail the way the product is broken. Now bound and covered by the
  composite-plan gate.
- **`f`/`t` were eating the second key of every sequence.** The find
  capture runs before the sequence stepper — correct for the FIRST key of
  a gesture, wrong for a later one — so `zt` armed a till-find and the
  `z` sequence never completed. Both operand-capture paths now decline
  while `pending_keys` is non-empty: by then the gesture has already been
  chosen.

**Ordering, which is now load-bearing in three places.** The dispatch
order is mark → object → find → sequence → keymap, and each step is a
real dependency rather than a convenience:

- **mark before object**, because the object path claims `i`/`a` whenever
  an operator is armed and a mark LETTER can be either — ``d`a`` lost its
  `a` to it. They do not fight over the first key: the mark path arms
  only while `pending_object` is clear, so `di'` still reaches the object
  path while `d'a` becomes a mark jump. Same key, two gestures,
  distinguished by which one is already half-typed.
- **object before find**, unchanged: `di(` must not read as `d`, insert,
  `(`.
- **both before the keymap**, and neither after the sequence stepper.

**Where `%`'s word-pair table stands.** Six languages are declared; only
`lua` and `sh` have a shipped `(defmode …)`, so `%` in Ruby, Bash,
Elixir and Vimscript is bracket-only today. The rows stay — the grammar
of `if`/`elsif`/`end` does not change when a major mode lands, and
deleting correct knowledge to shorten a list is how it gets re-derived
wrong — but `escriba/tests/matchit_filetypes.rs` pins the unreachable set
by SET EQUALITY, the same shape `action_resolution.rs` uses for
bound-but-inert keybinds. A new dead row fails; shipping the major mode
also fails until the row is promoted. Neither direction drifts quietly,
which is what makes "declared but unreachable" honest rather than wrong.

**Still not done:** `A-Z` and numbered marks, `` `` ``/`''` (the previous
position — the jumplist holds it, the mark map does not), marks surviving
edits above them (vim adjusts them; these are plain positions, clamped on
jump), `%` over HTML/XML tags, and `ge`/`b`/`<C-w>` remain single-line by
design — see the insert-mode note below for why widening them is not
free.

## `dd` worked and `p` did not exist (2026-08-13, fixed)

The operators captured text and the editor had **nowhere to put it back**
— `p`/`P` were unbound and `Action::Put` did not exist. `dd` and `yy`
were a delete key and a no-op with extra steps. Fixing that surfaced
four more defects, all in the same seam.

**The register is a TYPE now, not a `String`.** `escriba_core::Register`
= text + `RegisterKind::{Charwise, Linewise}`, and the KIND is what
chooses the put's behaviour — `p` after `dw` splices at a column, `p`
after `dd` opens a line. That had to land before `p` could exist at all:
with a `String` the put has to guess, and the only guess available drops
a terminated line into the middle of another. The kind comes from
`TextObject::register_kind()` (total over the enum, so a future `ip`/`ap`
paragraph object must decide) — never inferred from the range, because
`dd` on line 1 and a charwise motion produce the *same two positions*.
`Blockwise` is the arm vim has and escriba does not; when it lands it
will fail to compile at every consumer rather than silently pasting a
rectangle as a run of text.

**`dd`'s cursor was wrong twice, and every existing test was green** —
because every `dd` test asserted the TEXT. Deleting a line is easy to get
right, so the text was always correct while the cursor was not:

- It landed at **column 0** instead of the first non-blank. Untidy on
  flat prose; actively wrong on indented code, where it drops the cursor
  into the indentation and the next `i` types at the margin.
- On the last line it landed on the **phantom row** a trailing newline
  makes the rope report (see `last_text_line`). Nothing is there to edit,
  and the NEXT `dd` — issued from that row — took the "no following
  newline" branch and deleted the file's **terminator instead of a
  line**. The text shrank by one byte and the operator saw nothing
  happen. That is what "`dd` brings me to the top line" was: repeated
  `dd` near the end of a file walking the cursor up a line at a time,
  one line earlier than it should each round.

The rule is now vim's, in one place (`rest_after_operator`), keyed on the
KIND rather than on the operator, because that is what vim keys it on:
**first non-blank of the line that took the deleted line's place, never
past `last_text_line`.** `object_line_n` also clamps its extent to
`last_text_line` rather than to `line_count() - 1`, which makes a `dd`
issued FROM the phantom row resolve to an empty range instead of a
destructive one.

**`yy` moved the cursor and `yy` should not.** vim moves after a yank
only when the yank reached BACKWARDS; `yy`'s range starts at column 0, so
comparing POSITIONS read every `yy` as backwards and knocked the cursor
to the left margin. Invisible for `yw`, whose range starts exactly at the
cursor, so the move was a no-op there. A linewise yank compares LINES; a
charwise one compares positions. `yb` still rests at the start, so the
fix cannot be satisfied by "never move".

**Removal range ≠ register capture.** They are two views of one gesture
and they come apart on the last line of a file with no trailing newline:
there is no terminator to take forward, so the removal has to swallow the
PRECEDING one and the raw slice reads `"\nbravo"`. Right to remove, wrong
to put back. `as_linewise_capture` states the invariant once — a linewise
capture is newline-TERMINATED, never newline-LED — and
`Register::replayed` handles the other half (a capture that ends without
one), so `yyp` on an unterminated file gives two lines rather than
`bravobravo`.

**The counted forms were bypassing the bookkeeping entirely, and that is
the load-bearing find.** `3dw` / `2dd` / `3p` are ONE operation over an
`n`-fold extent, not `n` operations — the distinction is invisible for
delete (the text vanishes, so the repeat lands correctly by accident) and
wrong for yank (`2dd`'s register kept only the second line, so `2ddp` put
back half). The three absorbing arms used to short-circuit from
`apply_counted` **straight to their executors, skipping `apply_resolved`
altogether** — which is where damage classification and the dot-register
live. So `.` after any of them replayed nothing and the repaint span was
whatever the *previous* action had asked for. Now everything goes through
`apply_resolved(action, count)`; a free `absorbs_count()` says which arms
read the count instead of looping. `LastChange` records the real count
too, so `3dw` then `.` deletes three words rather than one.

`p`/`P` are pinned in `escriba/tests/movement_survives_defaults.rs` — a
bundled caixa taking `p` would be as invisible as the `<C-h>` shadowing
was — and the whole surface is in
`escriba-runtime/tests/linewise_and_put.rs` (26 key-driven tests, each
asserting cursor AND register AND text, because a wrong register KIND is
invisible to any one of the three alone).

**Still not done:** named registers (`"ay`), the numbered ring
(`"0`…`"9`), and the system clipboard — that last one is a *paste*, a
different verb over a different source (`hasami`, not this register),
which is why `Action::Put` is not called `Paste`. `2diw` also still
repeats rather than absorbing — the same over-count-a-yank defect,
waiting on a general "resolve this object n times"; `absorbs_count` names
the gap rather than half-fixing it.

## The single-key edit verbs (2026-08-13, landed)

`x X D C Y s S J gJ r` — the keys that turn the operators into an editor
you can actually type in. Pinned in
`escriba-runtime/tests/edit_verbs.rs` (39 tests).

**Five of the nine are pure keymap entries and ZERO new executor code**,
because they ARE operator-over-motion compositions that vim spells
shorter: `x`=`dl`, `X`=`dh`, `D`=`d$`, `C`=`c$`, `s`=`cl`, `S`=`cc`.
Binding them to the composed `Action::ApplyOperator` rather than giving
each its own variant is what makes counts, register capture, the linewise
cursor rule and dot-repeat all arrive for free and stay in step with the
long spelling forever — `edit_verbs.rs` asserts `x == dl` and `D == d$`
directly, because a divergence between a shortcut and its expansion goes
unnoticed until someone reaches for one of them in an edge case.

**The one prerequisite was clamping `Motion::Right` to its line.**
Unclamped, `dl` on an EMPTY line crossed the terminator, so `x` there
would have joined the next line on — a delete key that silently joins.
The cursor path was unaffected (`place_cursor` was already pulling `l`
back onto the last character), which is exactly why nothing had noticed.

**`Y` is `y$`, not `yy`.** The one key where vim and neovim actively
disagree — classic vim makes `Y` a synonym for `yy`, neovim ≥0.6 makes it
`y$`. escriba's default mirrors blnvim, which is neovim. Stated out loud
in the keymap and pinned in a test, because a silent choice here is a
trap for whichever half of the world guesses the other way.

**`cc`/`S` are fixed, and that needed a type.** A linewise CHANGE clears
the line's text and KEEPS the line — you are changing its contents, not
removing it — while a linewise DELETE takes the terminator too. Both
leave the same thing in the register. One `Range` cannot say that, so
`Extent` carries a **capture** range and a **removal** range plus the
kind. It also cleans up the older split: on the last line of a file with
no trailing newline the removal has to swallow the PRECEDING newline, so
it starts on a different LINE than the extent names, and every attempt to
derive one range from the other re-decides which branch produced it.
Charwise extents have `capture == removal`, which is why the old
single-range signature was right everywhere except here.

**`r` and `J` are the two with real executors, and both matter more for
what they REFUSE.** `5rx` on a three-character tail does *nothing* —
vim's rule, because a partial replace silently destroys characters you
did not name. `J` on the last line reports E36 rather than no-op:
a key that quietly does nothing is indistinguishable from an unbound one,
which is how `<C-h>` hid for a month. `J` also drops the next line's
indent and inserts one space, with vim's two exceptions (the line already
ends in whitespace; the next line starts with `)`); `gJ` splices verbatim
and exists precisely because `J` is lossy. A counted `J` is one
`Edit::replace` over the whole span, so `3J` is one `u`.

**`r` must stay UNBOUND, and that is a requirement rather than an
oversight.** Its operand is a KEY — `rw` must not read as `r` then *move
a word*, `ri` must not enter Insert — so `consume_replace_key` claims it
above the keymap, the same place `f`'s character and `` ` ``'s mark
letter are claimed. A binding on `r` would be a table entry no keypress
can reach: reads as configured, behaves as absent. `dr` is the one case
needing care — `r` is unbound, so it resolves to `Action::Pending`, and
the FSM deliberately lets a stray `Pending` leave an operator armed (for
multi-key sequences). Falling through would have left `d` armed and made
the next motion delete, so the capture cancels the operator explicitly.

**What wiring it surfaced — a FOURTH `<C-h>`-class shadowing.**
`escriba-leap` bound `s`/`S` (leap.nvim's own upstream choice, known to
be controversial there), so both core substitute verbs were dead in the
shipped build while every unit test stayed green. Same call as
`<S-h>`/`<S-l>`, `-` and `<C-h>` before it: leap is not wired, so it
traded two working edit verbs for two dead keys. Moved to
`<leader>s`/`<leader>S`. `movement_survives_defaults.rs` now gates the
edit verbs too, and separately asserts that `r` is **still unbound** —
so a caixa cannot quietly re-create the unreachable-binding trap either.

## unsoku is extracted, unwired, and quietly diverging (2026-08-13, found)

`unsoku` (運足) is a workspace member of THIS repo, published in
`[workspace.dependencies]`, and has **zero consumers** — nothing in escriba
says `use unsoku`. It was lifted out of escriba's semantics so a picker filter
box or a command palette could have `w`/`b`/`dw`/`ciw` without carrying a rope,
an LSP client and a plugin host. The lift happened; the reintegration never
did.

**This is not simple duplication, and calling it that would be wrong.** unsoku
resolves against **a single line** (`&str` + a `usize` caret); escriba-runtime
resolves against a **rope** (`Position { line, column }`). Those cannot share a
resolver, and unsoku's header says so. They already share the *vocabulary* —
both use `escriba_core::Motion` / `Operator` / `TextObject`, and unsoku
declares none of its own precisely to avoid that.

**What they should not have is two answers to one question, and they do.**
`unsoku::is_inclusive(motion)` and `escriba_core::Motion::is_inclusive()` are
two functions over the same enum, and they disagree on three arms:

| motion | `escriba_core` | `unsoku` |
|---|---|---|
| `MatchPair` (`%`) | inclusive | **not** |
| `LineEnd` (`$`) | not | **inclusive** |
| `DocEnd` (`G`) | not | **inclusive** |

Each disagreement is *defensible* on the representation: on one line `DocEnd`
IS `LineEnd`, and escriba reaches `d$`'s inclusive behaviour by resolving
`LineEnd` one-past-the-end instead of by flagging it. **None of it is written
down anywhere**, so the next reader finds two same-named functions over one
enum and no way to tell a deliberate divergence from a stale copy. That is the
defect — not the divergence itself.

**Before reaching for either:** `escriba_core::Motion::is_inclusive` is the
one the editor's operator path uses (`operated_end`). unsoku's is for
single-line consumers. If you change one, decide about the other in the same
commit, or write down why not.

**What today's work means for it.** The typed `Register` + `RegisterKind`
(charwise/linewise) landed in `escriba-runtime`, and unsoku's `Register` is
still `{ text: String }` with a `paste(after, count)` that has no linewise
notion. That is **correct as it stands** — a single-line target has no lines,
so linewise is meaningless there — and it is recorded here so the absence reads
as a decision rather than as lag.

`pending-unsoku: no-consumers — extracted 2026-08-09, still unwired`

## Insert mode could be typed into but not corrected (2026-08-09, fixed)

`Esc` was the ONLY binding `Mode::Insert` had, and `Keymap::dispatch`
answers `Key::Char` and `Key::Enter` *before* it consults the table. So
every other key — Backspace, `<Del>`, the arrows, Home/End — resolved to
`Action::Pending` and did nothing. **The executor was not missing; the
keys were unbound**, which is why the whole thing was invisible to the
unit tests: `Action::Backspace` was implemented and tested for prompts,
and nothing asked which KEY produced it in Insert.

Now bound, and the erase pair is ONE action per direction with three
targets — search prompt, ex line, buffer — routed by the runtime on typed
state it already owns. `Action::PromptBackspace`/`PromptDelete` were
renamed `Backspace`/`DeleteForward` to say so; `text_effect`'s doc had
described the buffer arm for months as though it existed. A face binding
`<BS>` should not have to know which of the three the operator is typing
into. Authorable as `:action "backspace"` / `"delete-forward"`.

Two things the buffer arm does NOT do, both deliberate: it does not route
through `apply_operator` (that captures the unnamed register, and erasing
a typo must not overwrite what you yanked), and it does not resolve
through `Motion::Left`/`Right` (those saturate at the line edge, so
column 0 would stop dead instead of joining the line above).

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

**The trap: a bundled caixa can shadow a core verb, and only the composite
plan shows it.** `<C-h>` was bound in `Keymap::default_vim()`, the unit
tests were green, `--keymap` printed it correctly — and pressing it in a
real editor did nothing. `escriba-luasnip`'s catalog entry bound `<C-h>`
to `snippet.jump-prev`, the shipped plan is applied ON TOP of the default
keymap, and `note_collisions` records a `Displaced` and binds anyway (an
rc IS allowed to override a default). The snippet engine is not wired, so
`<C-h>` traded a working erase key for a dead one. Every test in the repo
that touches keys reads `Keymap::default_vim()`, which was correct;
`escriba/tests/insert_erase_survives_defaults.rs` is the first to build
the keymap the BINARY boots with, and it fails the build if any caixa
takes an erase verb again. jump-prev moved to `<C-b>`. Found by driving
the real TUI in a pane and looking at the screen, not by reading code.

## One key translation per face, not two (2026-08-09, fixed)

`escriba-tui` carried its own crossterm→`Key` match in `keys.rs` while
`run.rs` carried an independent crossterm→`madori::KeyEvent` match, and
the event loop used the FIRST as a gate before feeding the runtime
through the SECOND. Two tables that had to agree and didn't: `Delete` and
the F-keys were in `run.rs`'s and absent from `keys.rs`'s, so the gate
dropped them before the runtime saw one. `<Del>` was bound to
`DeleteForward`, implemented, unit-tested — and **unreachable in the
default face**. `translate_crossterm_key` is now the composition of the
one crossterm-shaped match with `escriba_input::translate_key`, the same
function the GPU face uses; a key the two faces disagree about is no
longer constructible.

## The search caret was correct and invisible (2026-08-09, fixed)

`StatusModel::prompt_caret` carried the sentence *"a face draws its
cursor at `sigil_width + prompt_caret`"* for as long as **no face did**.
`←`/`→`/`Home`/`<C-w>` all moved the caret in the model, every unit test
agreed, and nothing on screen moved — so editing the middle of a pattern
was blind guesswork and the prompt never looked focused (ratatui hides
the cursor unless a frame asks for it). The sentence is now
`prompt_caret_offset()`, and the TUI parks the terminal cursor there,
measured off the spans it is actually painting rather than a hand-counted
column. Pinned in `escriba-tui/tests/status_line_frame.rs` against the
frame's cursor position. **The GPU face still does not draw a prompt
caret** — `pending-prompt-caret: gpu-face`.

> **★★★ CSE / Knowable Construction.** This repo operates under
> **Constructive Substrate Engineering** — canonical specification at
> [`pleme-io/theory/CONSTRUCTIVE-SUBSTRATE-ENGINEERING.md`](https://github.com/pleme-io/theory/blob/main/CONSTRUCTIVE-SUBSTRATE-ENGINEERING.md).
> The Compounding Directive (operational rules: solve once, load-bearing
> fixes only, idiom-first, models stay current, direction beats velocity)
> is in the org-level pleme-io/CLAUDE.md ★★★ section. Read both before
> non-trivial changes. Rust + tatara-lisp modal editor; the canonical
> `Rust owns invariants, Lisp owns authoring` application — 19 typed
> crates renderable to GPU/TUI/text targets from the same domain types.

Modal text editor written in Rust and authored in tatara-lisp.
pleme-io's canonical `Rust + Lisp` application of the editor category —
the same architectural decision Emacs makes with C+Elisp, Neovim makes
with C+Lua, and Zed makes with Rust+Rust — but targeting the
`Rust owns invariants, Lisp owns authoring` split the rest of the
pleme-io fleet already uses (frost, tatara, sui, etc.).

## Quick Start

```bash
cargo run -- scratch.txt            # open a file (default GPU window)
cargo run -- --render=tui file.rs   # ratatui inside any terminal
cargo run -- --render=text file.rs  # one-shot ANSI dump (CI / headless)
cargo run -- --commands             # list registered commands
cargo run -- --keymap               # list default keybindings
cargo run -- --spec > escriba.json  # dump OpenAPI 3.1 surface
cargo run -- plugin list            # 45 bundled plugin caixas + user installs
cargo run -- plugin forge --out o   # emit every plugin caixa dir (caixa.lisp + entry + flake)
cargo run -- --list-rc              # composite plan summary + wiring status
cargo run -- --no-splash            # skip the start screen for one run
cargo test --workspace --lib        # workspace unit tests (all green)
```

## Crate Map (19 crates)

| Crate | Purpose | Key types |
|-------|---------|-----------|
| `escriba-core` | Typed primitives — no I/O, no rendering | `Position`, `Range`, `Cursor`, `Selection`, `Mode`, `Motion`, `Operator`, `TextObject`, `Edit`, `Action`, `CountedAction`, `Register`, `RegisterKind`, `BufferId`, `WindowId` |
| `escriba-buffer` | Gap-buffer / rope backed text buffers, edits, undo | `Buffer`, `BufferSet`, `BufferError` |
| `escriba-config` | Config loading via shikumi | — |
| `escriba-mode` | Modal state machine (Normal / Insert / Visual / Command) with pending count + operator | `ModalState` |
| `escriba-keymap` | `(Mode, Key) → Action` binding table | `Key`, `Binding`, `Keymap` |
| `escriba-command` | Command registry + palette entries | `Command`, `CommandSpec`, `CommandRegistry`, `EditContext` |
| `escriba-api` | OpenAPI 3.1 surface generation | `OpenApiSpec`, `build_spec` |
| `escriba-spec` | Thin re-export of `escriba-api` | — |
| `escriba-ui` | Viewport, Window, Layout, Splash — pure layout math | `Viewport`, `Window`, `Rect`, `Layout`, `Splash` |
| `escriba-render` | Render backends (GPU via madori/garasu, text) | `Renderer`, `GpuRenderer`, `TextRenderer` |
| `escriba-tui` | ratatui + crossterm TUI backend | — |
| `escriba-input` | Platform-event → escriba-key translation | `InputOutcome`, `translate_app_event` |
| `escriba-runtime` | Editor state machine: `tick(event)` orchestration + lazy plugin host | `EditorState`, `PluginHost`, `LazyTrigger` |
| `escriba-plugin` | Plugin caixa model + **forge** (emit caixa.lisp / entry / flake from a catalog source) | `PluginCaixa`, `ActivationTrigger`, `forge_plugin`, `CaixaArtifacts` |
| `escriba-vm` | Embedded Lisp VM — skeleton for Lisp-authored logic | — |
| `escriba-ts` | Tree-sitter integration (incremental parse + highlight) | — |
| `escriba-lsp-client` | LSP client — hand-rolled framing, **not** tower-lsp (see below) | — |
| `escriba-mcp` | MCP server — expose editor state to AI agents | — |
| `escriba` | Binary — wires everything, owns CLI flags + render dispatch | — |

## LSP — what is wired, and the one thing that blocks the rest (2026-08-08)

**Corrected here: `escriba-lsp-client` is NOT tower-lsp-based.** It hand-rolls
its JSON-RPC framing in `wire.rs`. That is the deliberate fleet split — a
*server* takes `tower-lsp` (`caixa-lsp` set the precedent, `sui-lsp` follows),
a *client* does not, because tower-lsp's client half assumes it owns the
runtime and escriba's does not. The crate-map row said otherwise for a while;
anyone reaching for `tower_lsp::Client` here was going to have a bad afternoon.

### Wired and tested

| Piece | State |
|---|---|
| framing, connection, pending-request routing | `wire.rs` / `conn.rs` / `pending.rs`, 40 tests |
| positions | `zahyou`, re-exported — shared with `sui-lsp` so both ends of the wire do the same UTF-16 arithmetic |
| `publishDiagnostics` → `shirube::Finding` | `findings.rs`, 9 tests |
| `nix` → `sui-lsp` in the default `ServerRegistry` | `ServerConfig::sui_lsp()` |

The diagnostics adapter is a **source for the existing findings plane**, not a
second one: `escriba-shirube` already models a located finding, paints it in the
gutter, lists it and steps it with `]x`/`[x`, and its docs name diagnostics as
one of the seven producers it was built for — `Origin::Lsp` was already in the
enum. The runtime intent it needs, `Negai::PublishFindings`, already exists too.

The one genuinely hard part is that **LSP counts columns in UTF-16 code units
and `escriba_core::Position` counts them in `char`s.** Those agree on every
ASCII file and differ by one per astral-plane character, so a bridge that
assigns one to the other is correct in testing and wrong for anyone with an
emoji in a comment. `zahyou` keeps `Position` and `CharPosition` as distinct
types so that mistake is a compile error; a red-run confirms the pass-through
version reddens exactly one test and leaves eight green.

### The blocker was escriba's, not LSP's — and it is GONE

**CORRECTED 2026-08-13.** This section said, for as long as it existed, that
*"a live server cannot be pumped into the editor yet, because there is no async
delivery path into the runtime at all"*, that `Negai::Errand(_)` was
*"announced-but-unimplemented, waiting on the courier (Phase 5)"*, and that the
remaining work *was* the courier.

**The courier has landed.** `escriba-runtime/src/courier.rs` is `denrei` (伝令)
shipped: one `std::sync::mpsc` channel, a hired `Crew`, an errand counter, and
per-class cancel flags. `Negai::Errand(freight)` dispatches through it
(`escriba-runtime/src/lib.rs`), and `Negai::ErrandReply` applies replies gated
on `shirube::Anchor` freshness rather than on a second epoch authority.

The *no-`tokio`* half of the old claim is still true and is a DECISION, not a
gap — the courier is threads and channels, and `escriba-runtime` still has no
tokio dependency. Read that as "escriba does async without an async runtime",
not as "escriba cannot do async".

So the LSP pump is now the small thing it was always going to be: own a
`Connection`, classify incoming notifications, `to_findings`, emit
`Negai::PublishFindings { list: "diagnostics", .. }`. Every piece of that is
already written and tested — including, now, the channel.

**What is genuinely left is a SHAPE question, not a missing primitive.**
`Freight` is a closed three-variant enum and `Crew` a fixed three-runner
struct, both built for ONE-SHOT request→reply errands, with cancellation kept
*"newest per class"*. A language server is a long-lived, unsolicited producer
and there may be several. Whether that wants a fourth `Freight` variant, a
streaming freight class, or per-errand cancellation is the same fork
[`docs/doma.md` §6](./docs/doma.md) hits from the terminal side — and the
terminal is the better forcing function, because LSP can be faked with polling
and a PTY cannot.

`pending-lsp: live-pump — courier SHIPPED; remaining work is the streaming /
per-errand-cancel shape (see docs/doma.md §6)`

## Architecture

```
madori AppEvent ──► escriba-input ──► escriba-runtime.tick()
                                             ├── escriba-mode (state machine)
                                             ├── escriba-keymap (dispatch)
                                             ├── escriba-command (palette)
                                             ├── escriba-buffer (edits)
                                             └── escriba-ui (layout)
                                                   │
                                                   ▼
                                            EditorState
                                                   │
        ┌──────────────────┬───────────────────────┴───────────────────┐
        ▼                  ▼                                           ▼
   GpuRenderer        ratatui-tui                               TextRenderer
  (madori+garasu)   (crossterm)                             (ANSI-in-stdout)
```

External integrations live in sibling crates:
- `escriba-ts` — tree-sitter (incremental parse, highlight capture queries)
- `escriba-lsp-client` — LSP servers (rust-analyzer, gopls, typescript-language-server, …)
- `escriba-mcp` — AI agents via Model Context Protocol
- `escriba-plugin` — plugin caixa model + forge (see "Plugin Caixa Substrate")
- `escriba-vm` — embedded Lisp VM
- `escriba-lisp` — Tatara-Lisp authoring bridge (32 def-forms + `defescribaplugin` catalog form)

## ★ Plugin Caixa Substrate — generation-driven blnvim parity

**Canonical doc: [`docs/plugin-substrate.md`](./docs/plugin-substrate.md).**
**Every escriba plugin is a tatara-lisp caixa, emitted from one typed
catalog, installed by default, with Nix + per-plugin module support.**
This is Pillar 12 (generation over composition) + ★★ CLOSED-LOOP
MASS-SYNTHESIS + ★★ CATALOG REFLECTION applied to the editor-plugin
domain.

- **Authoring** — one `(defescribaplugin …)` source per plugin under
  `escriba/catalog/<name>.escribaplugin.lisp` (manifest form + the
  escriba entry def-forms). The manifest is inert at apply time, so the
  same file is both the spec and a valid plugin entry.
- **Forge** — `escriba_plugin::forge_plugin` emits each caixa's
  `caixa.lisp` (`:kind Biblioteca`) + `escriba/plugin.lisp` + `flake.nix`
  + the persisted spec, via the typed `escriba_lisp::sexp::Sexp` emitter
  (no string-concatenated lisp). `escriba plugin forge` / `install-bundled`
  materialize them.
- **45-caixa parity catalog** — every default-on blnvim capability is a
  caixa (lspconfig, conform, telescope, gitsigns, trouble, oil, cmp,
  treesitter + textobjects/fold/context/autotag, dap, neotest, illuminate,
  helm, lualine, bufferline, devicons, …; vim-tmux-navigator is absorbed
  by escriba-compass). The composite carries 12 LSP servers, 11
  formatters, 17 text objects, 5 DAP adapters, 23 icons, 58 highlights,
  93 keybinds.
- **Installed by default by construction** — the catalog is baked into
  the binary (`catalog_bundle::BUNDLED`, `include_str!`) and merged into
  the boot plan eagerly. No on-disk install, no network.
- **Solve once** — `configs/blnvim-defaults.lisp` is now the BASELINE
  only (theme, options, leader, major-modes, base highlights, escriba
  inventions). Plugin-owned forms (deflsp / defformatter / deftextobject
  / deffold / defdap / deficon / nord palette / statusline / bufferline /
  tasks / mcp) MIGRATED into their caixas — one concern, one home. A
  matrix test proves the migration lost nothing.
- **Lazy activation** — bundled defaults are eager; USER plugins in the
  plugins dir activate lazily via `escriba_runtime::PluginHost` (their
  entry applies on the first `Command` / `FileType` / `Event` trigger —
  the lazy.nvim model, typed).
- **Nix + modules** — each caixa ships a `flake.nix`; the HM/NixOS/Darwin
  trio exposes `programs.escriba.plugins.<name>.enable` (auto-discovered
  from the catalog dir), which sets `$ESCRIBA_DISABLED_PLUGINS` to omit a
  plugin's forms at boot.
- **Verification matrix** — `tests/plugin_matrix.rs` fails the build if a
  catalog file is malformed, mis-named, or missing from the baked table
  (dir↔table bijection), or if the composite loses capability.

**Adding a plugin** = drop a `catalog/<name>.escribaplugin.lisp` + add one
row to `catalog_bundle::BUNDLED` + `cargo test --test plugin_matrix`. The
substrate emits the caixa.lisp, the entry, and the flake mechanically.

## Embedding a terminal — `doma` (土間)

**[`docs/doma.md`](./docs/doma.md)** is the destination-first plan for making a
terminal a first-class citizen of escriba's pane tree. Status: **DESIGN, zero
code.** Three things from it worth knowing before anyone reaches for this:

- **You cannot embed mado, and should not want to.** mado is a BIN; its `[lib]`
  exposes only `motion` + `float` and says "do not widen this surface ad hoc".
  The world-fact underneath is that a terminal is a PTY + a VT grid + a pane
  registry — and the fleet ships all three as libraries in **`tear-core`**
  (`PaneGrid`, `Cell`, `PaneSnapshot`, `InProcess: MultiplexerControl`).
  tear-core's own doc names mado's rebase onto it as the destination; escriba
  joining is the same move, so the end state is ONE authoritative grid with
  three faces. Today there are two — `mado/src/terminal.rs` still owns its own
  `impl vte::Perform`.
- **The typed hole is one field.** `shikiri::Window { buffer_id: BufferId }` —
  a leaf IS a buffer, so a terminal pane is unrepresentable rather than merely
  unimplemented. `Nakami` (中身, "the contents") closes it, and every other gap
  is downstream.
- **`garasu::TextLayerStack` already supports N independent text layers and its
  doc names "terminal grid" as the motivating case**, so sharing one GPU device
  between the editor surface and a terminal grid is the DESIGNED shape rather
  than a hope. Unmeasured, though — as is the per-frame cost of
  `pane_snapshot()`, which is M0.1 and gates the read path's shape.

**Which terminal, and where it is going.** `doma` stands on `tear-core`
(3,253 lines; truecolor SGR, alt screen, DECSTBM, scrollback, proper
`unicode_width` double-width handling) — not on mado, which is a bin. Two
things move underneath it and are worth knowing before building on either:
`tear/docs/SHUKEN.md` decides `PaneGrid` becomes the sole VT authority with
all three original blockers cleared, and
[`theory/NATURALIZE-TERMINAL.md`](https://github.com/pleme-io/theory/blob/main/NATURALIZE-TERMINAL.md)
proposes that its dispatch table be GENERATED from a typed catalog rather than
hand-written (`masume`, design-tier, M0 green over 10 of ~1000 sequences).
Neither changes doma's seam: escriba drives panes through
`MultiplexerControl` either way.

`pending-doma: M0-measurement`

## Implementation plan for the backlog

**[`docs/backlog-plan.md`](./docs/backlog-plan.md)** is the ordered plan for
the inert actions plus Waves 1.5–4. It carries **no schedule** by operator
instruction (2026-08-07): the work goes piece by piece, ordered by what makes
the next piece safer, not by what is cheapest.

Three things from it that change how you should read the rest of this file:

- **The backlog was 5 missing primitives, not 22 subsystems** — `madoguchi` 窓口
  (dispatch seam), `shirube` 標 (located findings), `kasane` 重ね (floating
  surfaces), `shikiri` 仕切り (container tree), `denrei` 伝令 (the courier).
  **CORRECTED 2026-08-13: only `kasane` is still missing.** `madoguchi`,
  `shirube`, `shikiri` and `denrei` are all shipped crates/modules today
  (`escriba-madoguchi`, `escriba-shirube`, `escriba-ui/src/shikiri.rs` —
  516 lines, wired into `Layout` and `:sp`/`:vsp` — and
  `escriba-runtime/src/courier.rs`). The picker landed too
  (`escriba-ui/src/picker.rs` over `egaku::FuzzyPicker`), which the next
  bullet still lists as future work. **Re-read `docs/backlog-plan.md` against
  source before planning from it**; it has not been re-audited since these
  landed, and this file's own LSP section was stale in the same direction (see
  the correction above).
- **CORRECTED 2026-08-08 — "two of them land upstream in `egaku`" was FALSE.**
  `egaku::Modal` is a struct of `visible: bool` + `title: String`; `SplitPane`
  is exactly two panes and a ratio. Neither is a base for `kasane` or
  `shikiri` — both are net-new wherever they land. What egaku really offers
  escriba today is `FuzzyPicker<T>`, a tested typed `PickerEvent`→
  `PickerEffect<T>` machine with zero rendering deps (247 tests crate-wide,
  not 239), present in our `Cargo.lock` transitively with no escriba call
  sites. **`FuzzyPicker` specifically has no fleet consumer; egaku overall has
  five** — adoption risk is per-widget, not crate-wide. And its widgets were
  LIFTED OUT of mado, so these are proven designs whose migration never
  happened, not code nobody wanted.
- **Phase 0's three defects are FIXED**: `run_action` reported success for
  unknown actions, `run_command` discarded `NotFound`, and `BufferSet::open`
  silently duplicated an already-open file. All three now fail typed —
  `CommandError::Unhandled` and `BufferSet::find_by_path` respectively.

## Absorption Thesis

Escriba is the editor category distilled into typed primitives and
authored declaratively.

Every editor in the category is a solution to the same set of problems:
how to represent text, how to map keys to edits, how to extend the
system, how to integrate with external tooling. They differ in which
abstractions they commit to at each layer.

Escriba's plan is to absorb the best abstractions from each — typed in
Rust, composed in Lisp. The table below is the comparison matrix plus
what escriba does today and what it is absorbing next.

### Category Comparison Matrix

| Capability | vim / neovim | helix | kakoune | emacs | zed | vscode | sublime | cursor / windsurf | **escriba (today)** | **escriba (next)** |
|---|---|---|---|---|---|---|---|---|---|---|
| Text primitive | line-based | selection-first | selection-first | buffer | rope | line | rope | line | typed Position/Range | — |
| Buffer backing | gap buffer | rope (ropey) | rope | gap buffer | rope | array | rope | array | configurable | — |
| Modal editing | yes (vi) | yes (helix) | yes (kak) | no | no | no | no | no | yes (vi-like) | add helix noun-verb option |
| Multi-selection | weak | primary | primary | kill-ring only | primary | primary | primary | primary | single selection | **absorb: Selections (Vec<Selection>)** |
| Registers / kill-ring | `"a…"z` + `"0`…`"9` | `"` + `_` | `"` + `*` | kill-ring | clipboard | clipboard | clipboard | clipboard | unnamed register, typed charwise/linewise, `p`/`P` | **absorb: named registers `"ay`, `"0`…`"9`, system clipboard via `hasami`** |
| Marks / jumplist | `m[a-z]` + jumplist | jumplist | marks | marks + registers | — | — | — | — | — | **absorb: Marks + Jumplist** |
| Tree-sitter | yes (plugin) | yes (built-in) | no | tree-sitter.el | yes (built-in) | yes | no | yes | yes (`escriba-ts`) | expand captures + folds |
| LSP client | yes (plugin / built-in 0.5) | yes (built-in) | lsp-kak plugin | lsp-mode / eglot | yes (built-in) | yes (built-in) | LSP plugin | yes | yes (`escriba-lsp-client`) | enrich (inlay hints, semantic tokens, workspace symbols) |
| DAP client | nvim-dap | — | dap-kak plugin | dape.el | debug | yes (built-in) | — | yes | — | **absorb: escriba-dap-client** |
| Scripting language | vimscript + lua | none (yet) | shell | elisp | TS extensions | TS extensions | Python | TS | tatara-lisp (via `escriba-vm`) | **absorb: escriba-lisp authoring bridge** |
| Package / plugin manager | vim-plug, lazy | — | plug.kak | package.el, elpaca | extensions (Rust+WASM) | marketplace | Package Control | marketplace | `escriba-plugin` scaffold | plugin manifest declared in Lisp |
| Command palette | `:` | `:` + picker | `:` | `M-x` | Cmd-Shift-P | Cmd-Shift-P | Cmd-Shift-P | Cmd-Shift-P | `escriba-command` registry | wire palette UI + `egaku::fuzzy_score` |
| Fuzzy picker | telescope, fzf-lua | built-in | fzf plugin | helm, vertico | built-in | quick open | goto anything | built-in | — | **absorb: escriba-picker (wraps `egaku::FuzzyPicker<T>`)** |
| Which-key prompt | which-key.nvim | built-in | — | which-key.el | partial | partial | — | — | — | **absorb: WhichKey popup** |
| Status line | lualine / lightline | built-in | status-line | mode-line | statusbar | status bar | status bar | status bar | basic | customizable via Lisp |
| Tab / buffer line | bufferline.nvim | — | — | tab-bar-mode | tab bar | tabs | tabs | tabs | basic | — |
| File tree | nvim-tree | — | — | dired, treemacs | file tree | explorer | sidebar | explorer | — | **absorb: escriba-tree** |
| Git integration | fugitive, gitsigns | built-in gutter | — | magit | built-in | GitLens | git gutter | built-in | — | **absorb: escriba-git (reuse `git2`)** |
| Integrated terminal | `:terminal` | — | — | ansi-term, eat, vterm | built-in | built-in | — | built-in | — | **absorb: escriba-term (embed mado/frost)** |
| AI inline assist | copilot.lua | — | — | gptel | zed-ai | copilot | LLM plugin | primary | `escriba-mcp` server | **absorb: MCP client + inline completion** |
| Snippets | luasnip | — | — | yasnippet | snippets | snippets | snippets | snippets | — | snippet spec in Lisp |
| Folding | `foldmethod=syntax/treesitter` | — | — | origami.el, hs-mode | built-in | built-in | — | built-in | — | tree-sitter folds |
| Undo tree | undotree.vim | — | — | undo-tree.el | linear | linear | linear | linear | basic undo | persistent undo tree |
| Macros | `q` / `Q` | — | — | kbd-macros | — | — | — | — | — | record keys → action seq |
| Collaboration | — | — | — | crdt.el | primary | Live Share | — | primary | — | — |
| Notebook cells | notebook.nvim | — | — | org-babel, jupyter | repl | Jupyter | — | — | — | — |
| Session / layout persistence | session.vim | — | — | desktop.el | workspaces | workspaces | projects | workspaces | — | layout serializer |
| Minimap | — | — | — | minimap.el | yes | yes | yes (iconic) | yes | — | optional |

### Conclusions

1. **Escriba is on the correct axis for the Rust+Lisp door.** The base
   modal model (`escriba-mode`), typed primitives (`escriba-core`), and
   command/keymap registries match how Emacs, Neovim, and Helix model
   the editor; what's missing is the Lisp authoring bridge that maps
   the Rust state onto Lisp surfaces users write by hand.
2. **Multi-selection is the biggest capability gap vs. modern
   editors** (Helix, Kakoune, Sublime, VSCode). The `Selection` type
   needs to be plural (`Vec<Selection>`), and every motion / operator
   needs to apply to the set.
3. **The picker UI is the second biggest gap.** Every capable editor
   has fuzzy finding (Telescope, Helix picker, Cmd-Shift-P, Goto
   Anything). **The fleet's answer is `egaku::FuzzyPicker<T>`** — 864
   lines, a typed `PickerEvent`→`PickerEffect<T>` machine with its own
   `fuzzy_score`, already in escriba's `Cargo.lock` as a transitive dep and
   with zero rendering dependencies. Verified 2026-08-08: escriba has no
   direct egaku dependency and no call sites, so adopting it is additive. Ship `escriba-picker` as an adapter
   over it. (This previously said `skim`, "already in the fleet via
   frostmourne" — VERIFIED FALSE 2026-08-07: no fleet lockfile contains
   skim and frostmourne has no Rust crate. Building on that claim would
   have added a dependency to replace a library we already ship.)
4. **AI pair programming is the clearest moat.** escriba has `escriba-mcp`
   as a server but no MCP *client* inside the editor. An MCP client
   + inline-completion widget would let escriba be a Cursor-style
   agentic editor without leaving the Rust+Lisp pattern.
5. **escriba-term is a composition win.** mado (GPU terminal) and
   frost (zsh-compatible shell) already exist in pleme-io — an editor
   that embeds them as a splittable term pane reuses 100% of that
   work.

## The `escriba-lisp` bridge

**Status: planned — this is the first absorption PR after this
CLAUDE.md.** Mirrors [`frost-lisp`](https://github.com/pleme-io/frost/tree/main/crates/frost-lisp)
one-for-one.

Intent: every piece of editor state that today lives in a `default_*()`
factory should be declarable via Tatara-Lisp. Config composes across
multiple forms the same way frost-lisp composes across multiple
`.lisp` files.

Initial form set:

```lisp
;; bind a key in a mode
(defkeybind :mode "normal" :key "gh" :action "goto-home")
(defkeybind :mode "insert" :key "jk" :action "escape-to-normal")

;; register a command (wired into the command palette)
(defcmd :name "write-all"  :description "Write every modified buffer"
        :action "buffer.write-all")

;; toggle/set an editor option
(defoption :name "number"          :value "true")
(defoption :name "tabstop"         :value "4")
(defoption :name "relativenumber"  :value "true")

;; select a theme (reuses irodzuki palette, mirrors frost-lisp deftheme)
(deftheme :preset "nord")

;; hook on an editor event
(defhook :event "BufWritePost" :command "run-formatter")
(defhook :event "ModeChanged" :to "insert" :command "highlight-cursor-line")

;; filetype routing (extension → mode)
(defft :ext "rs" :mode "rust")

;; abbreviation (insert-mode auto-expand)
(defabbrev :trigger "teh" :expansion "the")

;; snippet
(defsnippet :trigger "fn" :body "fn ${1:name}(${2}) -> ${3} { ${0} }")
```

Every spec is a `#[derive(DeriveTataraDomain)]` struct; the bridge
exposes `load_rc(path, &mut EditorState) -> ApplySummary` the binary
calls at startup. The binary gains a `--rc <path>` flag and honors
`$ESCRIBARC` like frost honors `$FROSTRC`.

## Escribamourne (future — the curated escriba distribution)

Planned analogue of `frostmourne` — a flake that bundles escriba plus
a batteries-included Lisp-authored configuration covering sensible
keybindings, themes, LSP server setup, AI assistant wiring, and
integrated terminal. Out of scope for this session; will be a new
repo (`pleme-io/escribamourne`) once `escriba-lisp` has enough surface
to configure meaningfully.

## Absorption Roadmap

The absorption roadmap is a DAG, not a line — each group can proceed
independently. Ordered by impact × effort ratio:

**Wave 1 — Authoring bridge (landed):**

1. ✅ `escriba-lisp` crate — 8 def-forms (`defkeybind`, `defcmd`,
   `defoption`, `deftheme`, `defhook`, `defft`, `defabbrev`,
   `defsnippet`), each a `#[derive(DeriveTataraDomain)]` spec.
2. ✅ Binary wires `--rc <path>` + `$ESCRIBARC` + `--list-rc`,
   loads and applies at startup.
3. ✅ Apply layer: `apply_plan_to_keymap` resolves the key-string
   grammar (`<Esc>`, `<C-r>`, `<A-f>`, named keys, bare chars),
   maps 26 well-known action strings to typed `Action` variants,
   and falls back to `Action::Command { name, args }` for
   anything else (plugin-registered commands resolve later).
4. ✅ Integration harness (`escriba/tests/rc_integration.rs`) +
   sample fixture (`escriba/examples/sample-rc.lisp`) — 115 tests
   covering parse, apply, and end-to-end binary flow.

**Wave 1.5 — Authoring-bridge polish (queued):**

1. Multi-key sequences (`gh`, `gg`, `dd`) via pending-stroke state
   in `ModalState`. Currently surfaced as a warning from the apply
   layer. Requires keymap + runtime changes.
2. More apply paths:
   `apply_plan_to_commands` (defcmd → CommandRegistry),
   `apply_plan_to_options` (defoption → new `EditorOptions` slot),
   `apply_plan_to_hooks` (defhook → new autocmd dispatcher).
3. `defsource :path "other.lisp"` — rc composition across files,
   mirroring frost-lisp's `defsource`.

**Wave 2 — UX essentials:**

1. **Multi-selection.** Promote `Cursor` → `Cursors` (Vec<Position>)
   and `Selection` → `Selections` (Vec<Selection>) in `escriba-core`.
   Every motion / operator maps over the set. Selection-first mode
   (Helix-style) becomes a Lisp-selectable preset.
2. **escriba-picker.** An adapter over `egaku::FuzzyPicker<T>` (NOT
   skim — see the Conclusions note above). Files, buffers, commands,
   symbols are one adapter over different `T`.
3. **Which-key popup.** Multi-stroke binding preview — render the
   pending key prefix's completions after a configurable delay.
4. **Registers / clipboard.** `"a…"z` registers, kill-ring, system
   clipboard via `hasami`.
5. **Marks / jumplist.** `m[a-z]`, `''`, `C-o`, `C-i` parity.

**Wave 3 — Tooling integrations:**

1. **escriba-dap-client.** DAP parallel to `escriba-lsp-client`.
   Breakpoints, stepping, watches.
2. **escriba-git.** Git integration — `git2` crate, gutter signs,
   blame, log, diff view.
3. **escriba-term.** Embedded terminal pane. Uses mado + frost's
   existing primitives.
4. **escriba-tree.** File tree sidebar.
5. **escriba-lsp-client** enrichment — inlay hints, semantic tokens,
   workspace symbols, code actions, rename, signature help.
6. **AI inline assist** — MCP *client* that calls Claude for inline
   completions, edits, chat. `escriba-mcp` already has the server
   side; pair with the client.

**Wave 4 — Advanced:**

1. **Undo tree** — persistent, branch-able.
2. **Macros** — key-sequence record / replay, storable as Lisp.
3. **Snippets** — declared via `defsnippet`.
4. **Folding** — tree-sitter-aware.
5. **Session persistence** — layout + buffer positions serialized.
6. **Notebook cells** — tree-sitter-driven cell detection.
7. **Collaboration** — CRDT, pair with zed's architecture.

## Conventions

- Edition 2024, Rust 1.89.0+, MIT license.
- `cargo build --workspace` must be warning-free.
- Every new crate wires into `Cargo.toml [workspace.dependencies]` with
  a `version` field so crates.io publishing works later.
- Every `#[tatara(keyword = "…")]` spec needs a dedicated pass in
  `escriba-lisp::apply_source` that mutates `EditorState`.
- Never hand-write a `default_*()` factory that could be a Lisp form.
  If it's configurable, it's a def-form.
