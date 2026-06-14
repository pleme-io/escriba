;; escriba — Vellum theme (the fleet default)
;; ------------------------------------------------------------------
;; Vellum is the pleme-io fleet theme: warm aged-paper Nord-matte. An
;; aged-parchment ground (warm, low-chroma) with muted matte ink above
;; it. Mirrors `ishou_tokens::FleetTheme::Vellum` /
;; `VellumPalette::vellum()` — every hex here is a BORN ishou token, so
;; the editor chrome + syntax match the fleet (mado, tear, frostmourne,
;; namimado, …).
;;
;; Load it explicitly:
;;
;;   escriba --rc escriba/configs/vellum.lisp <file>
;;
;; Or as a standalone theme module alongside your own rc. The
;; `blnvim-defaults.lisp` config already selects this theme by default
;; via `(deftheme :preset "vellum")`.
;;
;; Base16 + role hexes (ishou VellumPalette):
;;   base00 #16140E (night0/bg)   base01 #1F1C15 (night1/surface)
;;   base02 #2B2820 (night2/sel)  base03 #90897B (shadow1/comment)
;;   base04 #ADA593 (snow0/dim)   base05 #E2DBC8 (snow1/fg)
;;   base06 #EDE6D6 (snow2)       base07 #F4EFE2 (snow3)
;;   base08 #C9837B (aurora_red)  base09 #CB9070 (ember/orange)
;;   base0A #D7C489 (first_light) base0B #A9BB8C (aurora_green)
;;   base0C #94BBB8 (ice_cyan)    base0D #99AABE (ice_steel)
;;   base0E #B8A1B9 (solar_mag)   base0F #B3886C (dusk_bronze)
;;   cursor #ADD7A3 (green_bright)

;; ═════ Theme select ═══════════════════════════════════════════════
(deftheme :preset "vellum")

;; ═════ Palette — vellum canonical values (ishou base16) ═══════════
(defpalette :name "vellum"
            :base00 "#16140e" :base01 "#1f1c15" :base02 "#2b2820"
            :base03 "#90897b" :base04 "#ada593" :base05 "#e2dbc8"
            :base06 "#ede6d6" :base07 "#f4efe2"
            :base08 "#c9837b" :base09 "#cb9070" :base0a "#d7c489"
            :base0b "#a9bb8c" :base0c "#94bbb8" :base0d "#99aabe"
            :base0e "#b8a1b9" :base0f "#b3886c")

;; ═════ Highlights — vellum syntax groups ══════════════════════════
;; Covers the canonical syntax + UI + diagnostic + git groups
;; (`CANONICAL_GROUPS` in escriba-lisp/src/highlight.rs).

;; ── Syntax ────────────────────────────────────────────────────────
(defhighlight :group "Normal"     :fg "#e2dbc8" :bg "#16140e")
(defhighlight :group "Comment"    :fg "#90897b" :italic #t)
(defhighlight :group "String"     :fg "#a9bb8c")
(defhighlight :group "Number"     :fg "#b8a1b9")
(defhighlight :group "Boolean"    :fg "#b8a1b9")
(defhighlight :group "Function"   :fg "#99aabe" :bold #t)
(defhighlight :group "Keyword"    :fg "#b8a1b9" :italic #t)
(defhighlight :group "Statement"  :fg "#b8a1b9")
(defhighlight :group "Conditional" :fg "#b8a1b9")
(defhighlight :group "Repeat"     :fg "#b8a1b9")
(defhighlight :group "Operator"   :fg "#b8a1b9")
(defhighlight :group "Type"       :fg "#d7c489")
(defhighlight :group "Structure"  :fg "#d7c489")
(defhighlight :group "Identifier" :fg "#e2dbc8")
(defhighlight :group "Constant"   :fg "#cb9070")
(defhighlight :group "PreProc"    :fg "#cb9070")
(defhighlight :group "Macro"      :fg "#cb9070")
(defhighlight :group "Special"    :fg "#d7c489")

;; ── UI ────────────────────────────────────────────────────────────
(defhighlight :group "CursorLine"   :bg "#1f1c15")
(defhighlight :group "CursorColumn" :bg "#1f1c15")
(defhighlight :group "LineNr"       :fg "#90897b")
(defhighlight :group "SignColumn"   :bg "#16140e")
(defhighlight :group "Visual"       :bg "#2b2820")
(defhighlight :group "VisualNOS"    :bg "#2b2820")
(defhighlight :group "Search"       :fg "#16140e" :bg "#d7c489")
(defhighlight :group "IncSearch"    :fg "#16140e" :bg "#cb9070" :bold #t)
(defhighlight :group "MatchParen"   :fg "#cb9070" :bold #t)
(defhighlight :group "StatusLine"   :fg "#cdc7b6" :bg "#1f1c15")
(defhighlight :group "StatusLineNC" :fg "#90897b" :bg "#16140e")
(defhighlight :group "TabLine"      :fg "#90897b" :bg "#16140e")
(defhighlight :group "TabLineFill"  :bg "#16140e")
(defhighlight :group "TabLineSel"   :fg "#16140e" :bg "#94bbb8" :bold #t)
(defhighlight :group "VertSplit"    :fg "#38342a")
(defhighlight :group "Pmenu"        :fg "#e2dbc8" :bg "#1f1c15")
(defhighlight :group "PmenuSel"     :fg "#16140e" :bg "#94bbb8" :bold #t)
(defhighlight :group "PmenuSbar"    :bg "#1f1c15")
(defhighlight :group "PmenuThumb"   :bg "#90897b")
(defhighlight :group "NormalFloat"  :fg "#e2dbc8" :bg "#1f1c15")
(defhighlight :group "FloatBorder"  :fg "#99aabe" :bg "#1f1c15")

;; ── Diagnostics ───────────────────────────────────────────────────
(defhighlight :group "DiagnosticError" :fg "#c9837b" :bold #t)
(defhighlight :group "DiagnosticWarn"  :fg "#d7c489")
(defhighlight :group "DiagnosticInfo"  :fg "#94bbb8")
(defhighlight :group "DiagnosticHint"  :fg "#a9bb8c")

;; ── Git (gitsigns.nvim parity) ────────────────────────────────────
(defhighlight :group "GitSignsAdd"    :fg "#a9bb8c")
(defhighlight :group "GitSignsChange" :fg "#d7c489")
(defhighlight :group "GitSignsDelete" :fg "#c9837b")
(defhighlight :group "DiffAdd"        :bg "#4d543e")
(defhighlight :group "DiffChange"     :bg "#595137")
(defhighlight :group "DiffDelete"     :bg "#7b4f4a")
(defhighlight :group "DiffText"       :bg "#5d6773")

;; ── Tree-sitter semantic overrides ────────────────────────────────
(defhighlight :group "@function.call" :link "Function")
(defhighlight :group "@variable"      :link "Identifier")
(defhighlight :group "@parameter"     :fg "#e2dbc8" :italic #t)
(defhighlight :group "@comment.todo"  :fg "#d7c489" :bold #t)
(defhighlight :group "@comment.note"  :fg "#94bbb8" :bold #t)
(defhighlight :group "@comment.warning" :fg "#cb9070" :bold #t)
