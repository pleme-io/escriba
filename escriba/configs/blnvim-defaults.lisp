;; escriba — blnvim-compatible BASELINE
;; ----------------------------------------
;; This is the editor BASELINE that escriba boots with (unless
;; `--no-defaults`). It carries the non-plugin substrate: theme, core
;; `:set` options, the leader, baseline motion keybinds, language
;; major-modes, the base syntax + UI highlight groups, the fleet
;; palette, and escriba's convergence-layer inventions (gates,
;; workflows, sessions, effects, terms, marks, schedules, kmacros,
;; attests, rulers).
;;
;; Everything that is a PLUGIN — LSP, completion, telescope, git,
;; formatters, treesitter text-objects + folds, DAP, statusline,
;; bufferline, icons, the Nord palette, tasks, MCP bindings, … — now
;; lives in its own installable caixa under `catalog/` and is applied
;; ON TOP of this baseline at boot (see `escriba plugin list`). One
;; concern, one home: a deflsp/deformatter/deftextobject form lives in
;; exactly ONE place. This file no longer duplicates them.
;;
;; Load it explicitly:   escriba --rc escriba/configs/blnvim-defaults.lisp <file>
;; Or copy into place:   cp … ~/.escribarc.lisp

;; ═════ Theme ══════════════════════════════════════════════════════
;; The fleet-prescribed theme — `ishou_tokens::FleetTheme::prescribed_default()`,
;; today PlemeDark, whose operator-facing spelling is "nord". This is what
;; the two `convergence::Guard` tests enforce escriba must paint.
;;
;; This line USED to say "vellum", left over from before the fleet moved,
;; and nothing caught it because nothing consumed it: every paint site
;; called `ChromePalette::prescribed()` for itself, so the editor painted
;; Nord regardless of what was declared here. Wiring `(deftheme …)` into
;; the paint path is what turned the stale declaration into a visible
;; contradiction — `theme_declaration_agrees_with_the_fleet_default` now
;; keeps the two from drifting apart again.
;;
;; Vellum is still shipped, as an opt-in: `--rc escriba/configs/vellum.lisp`.
(deftheme :preset "nord")

;; ═════ Options ════════════════════════════════════════════════════
;; Mirror the `:set` commands every blnvim user gets implicitly.
(defoption :name "number"          :value "true")
(defoption :name "relativenumber"  :value "true")
(defoption :name "cursorline"      :value "true")
(defoption :name "expandtab"       :value "true")
(defoption :name "tabstop"         :value "2")
(defoption :name "shiftwidth"      :value "2")
(defoption :name "smartindent"     :value "true")
(defoption :name "wrap"            :value "false")
(defoption :name "hlsearch"        :value "true")
(defoption :name "incsearch"       :value "true")
(defoption :name "ignorecase"      :value "true")
(defoption :name "smartcase"       :value "true")
(defoption :name "termguicolors"   :value "true")
(defoption :name "splitright"      :value "true")
(defoption :name "splitbelow"      :value "true")
(defoption :name "signcolumn"      :value "yes")
(defoption :name "updatetime"      :value "300")
(defoption :name "scrolloff"       :value "8")
(defoption :name "sidescrolloff"   :value "8")
(defoption :name "timeoutlen"      :value "300")
(defoption :name "undofile"        :value "true")

;; ═════ Leader ═════════════════════════════════════════════════════
;; The blnvim leader is COMMA (`vim.g.mapleader = ","` — package/init.lua:5).
;; `<leader>` resolves at bind time against this option — set FIRST so every
;; `<leader>…` sequence (here + in the catalog plugins) binds under the right
;; prefix. Matched to blnvim so an operator's muscle memory carries over.
(defoption :name "mapleader"       :value ",")
(defoption :name "maplocalleader"  :value ",")

;; ═════ Baseline keybindings ═══════════════════════════════════════
;; Non-plugin editor motions + buffer/file ops. Plugin keybinds
;; (picker.*, lsp.*, git.*, trouble.*, files.*, pane.*) live in their
;; catalog caixas.
(defkeybind :mode "normal" :key "<C-d>" :action "half-page-down"
            :description "half page down")
(defkeybind :mode "normal" :key "<C-u>" :action "half-page-up"
            :description "half page up")
;; ── Windows — `:sp` / `:vsp` and the <C-w> family ──────────────────
;;
;; `<C-w>` is free in Normal mode (it is bound in COMMAND mode to
;; PromptDeleteWord, which is vim's behaviour too and does not collide).
;; These are the bindings an operator's hands already know; anything past
;; them (`<C-w>+`/`-`/`<`/`>` resizing, `<C-w>o` only) is deliberately not
;; here yet — see docs/backlog-plan.md.
(defkeybind :mode "normal" :key "<C-w>s" :action "window.split"
            :description "split horizontally")
(defkeybind :mode "normal" :key "<C-w>v" :action "window.vsplit"
            :description "split vertically")
(defkeybind :mode "normal" :key "<C-w>c" :action "window.close"
            :description "close this window")
(defkeybind :mode "normal" :key "<C-w>h" :action "pane.left"
            :description "focus the window left")
(defkeybind :mode "normal" :key "<C-w>j" :action "pane.down"
            :description "focus the window below")
(defkeybind :mode "normal" :key "<C-w>k" :action "pane.up"
            :description "focus the window above")
(defkeybind :mode "normal" :key "<C-w>l" :action "pane.right"
            :description "focus the window right")

;; The ex-command spellings. `:sp` and `:vsp` are what fingers type.
(defcmd :name "sp"     :description "Split the window horizontally"
        :action "window.split")
(defcmd :name "split"  :description "Split the window horizontally"
        :action "window.split")
(defcmd :name "vsp"    :description "Split the window vertically"
        :action "window.vsplit")
(defcmd :name "vsplit" :description "Split the window vertically"
        :action "window.vsplit")
(defcmd :name "close"  :description "Close the active window"
        :action "window.close")

(defkeybind :mode "normal" :key "<C-s>" :action "save"
            :description "write the active buffer (blnvim <C-s>)")
(defkeybind :mode "normal" :key "<leader>bd" :action "buffer.delete"
            :description "close the current buffer")
(defkeybind :mode "normal" :key "<leader>w" :action "save"
            :description "write the active buffer")
(defkeybind :mode "normal" :key "<leader>q" :action "quit"
            :description "quit the editor")

;; ═════ Major modes ════════════════════════════════════════════════
;; Language registry — ftdetect + tree-sitter grammar + indent. Matches
;; the grammars blnvim ships by default.
(defmode :name "rust"       :extensions ("rs")        :tree-sitter "rust"       :commentstring "// %s" :indent 4)
(defmode :name "nix"        :extensions ("nix")       :tree-sitter "nix"        :commentstring "# %s"  :indent 2)
(defmode :name "lisp"       :extensions ("lisp" "cl" "el") :tree-sitter "commonlisp" :commentstring ";; %s" :indent 2 :structural-lisp #t)
(defmode :name "python"     :extensions ("py")        :tree-sitter "python"     :commentstring "# %s"  :indent 4)
(defmode :name "javascript" :extensions ("js" "mjs")  :tree-sitter "javascript" :commentstring "// %s" :indent 2)
(defmode :name "typescript" :extensions ("ts" "tsx")  :tree-sitter "typescript" :commentstring "// %s" :indent 2)
(defmode :name "go"         :extensions ("go")        :tree-sitter "go"         :commentstring "// %s" :indent 4)
(defmode :name "lua"        :extensions ("lua")       :tree-sitter "lua"        :commentstring "-- %s" :indent 2)
(defmode :name "markdown"   :extensions ("md")        :tree-sitter "markdown"   :commentstring "<!-- %s -->" :indent 2)
(defmode :name "yaml"       :extensions ("yaml" "yml") :tree-sitter "yaml"      :commentstring "# %s"  :indent 2)
(defmode :name "toml"       :extensions ("toml")      :tree-sitter "toml"       :commentstring "# %s"  :indent 2)
(defmode :name "json"       :extensions ("json")      :tree-sitter "json"       :indent 2)
(defmode :name "sh"         :extensions ("sh" "bash") :tree-sitter "bash"       :commentstring "# %s"  :indent 2)
(defmode :name "terraform"  :extensions ("tf" "tfvars") :tree-sitter "hcl"      :commentstring "# %s"  :indent 2)

;; blue — the pleme-io language (Ruby/Elixir surface, tatara-lisp AST,
;; Rust runtime). No `:tree-sitter`, and that omission is the honest
;; part: there is no tree-sitter-blue grammar anywhere, and naming one
;; would make `apply_grammars` count `.b` as an extension skipped for an
;; unknown language. `.b` highlighting is real and comes from the other
;; direction — escriba-render registers a hikari table plugin for it
;; (`escriba_render::langs`), which is where every non-tree-sitter
;; language in this list is served from too.
(defmode :name "blue"       :extensions ("b")         :commentstring "# %s"  :indent 2)

;; ═════ Highlights — base syntax + UI (vellum palette) ═════════════
;; Plugin-specific groups (Diagnostic*, GitSigns*, @comment.todo, …)
;; live in their catalog caixas; these are the language-agnostic base.
(defhighlight :group "Normal"     :fg "#e2dbc8" :bg "#16140e")
(defhighlight :group "Comment"    :fg "#90897b" :italic #t)
(defhighlight :group "String"     :fg "#a9bb8c")
(defhighlight :group "Number"     :fg "#b8a1b9")
(defhighlight :group "Boolean"    :fg "#b8a1b9")
(defhighlight :group "Function"   :fg "#99aabe" :bold #t)
(defhighlight :group "Keyword"    :fg "#b8a1b9" :italic #t)
(defhighlight :group "Statement"  :fg "#b8a1b9")
(defhighlight :group "Operator"   :fg "#b8a1b9")
(defhighlight :group "Type"       :fg "#d7c489")
(defhighlight :group "Identifier" :fg "#e2dbc8")
(defhighlight :group "Constant"   :fg "#cb9070")
(defhighlight :group "Special"    :fg "#d7c489")

;; ── UI ────────────────────────────────────────────────────────────
(defhighlight :group "CursorLine"   :bg "#1f1c15")
(defhighlight :group "LineNr"       :fg "#90897b")
(defhighlight :group "SignColumn"   :bg "#16140e")
(defhighlight :group "Visual"       :bg "#2b2820")
(defhighlight :group "Search"       :fg "#16140e" :bg "#d7c489")
(defhighlight :group "IncSearch"    :fg "#16140e" :bg "#cb9070" :bold #t)
(defhighlight :group "MatchParen"   :fg "#cb9070" :bold #t)
(defhighlight :group "StatusLine"   :fg "#cdc7b6" :bg "#1f1c15")
(defhighlight :group "StatusLineNC" :fg "#90897b" :bg "#16140e")
(defhighlight :group "Pmenu"        :fg "#e2dbc8" :bg "#1f1c15")
(defhighlight :group "PmenuSel"     :fg "#16140e" :bg "#94bbb8" :bold #t)
(defhighlight :group "NormalFloat"  :bg "#1f1c15")
(defhighlight :group "FloatBorder"  :fg "#99aabe" :bg "#1f1c15")

;; ── Tree-sitter semantic (language-agnostic) ──────────────────────
(defhighlight :group "@function.call" :link "Function")
(defhighlight :group "@variable"      :link "Identifier")
(defhighlight :group "@parameter"     :fg "#e2dbc8" :italic #t)

;; ═════ Palette — vellum (fleet default) ═══════════════════════════
;; The Nord palette lives in the escriba-nord caixa.
(defpalette :name "vellum"
            :base00 "#16140e" :base01 "#1f1c15" :base02 "#2b2820"
            :base03 "#90897b" :base04 "#ada593" :base05 "#e2dbc8"
            :base06 "#ede6d6" :base07 "#f4efe2"
            :base08 "#c9837b" :base09 "#cb9070" :base0a "#d7c489"
            :base0b "#a9bb8c" :base0c "#94bbb8" :base0d "#99aabe"
            :base0e "#b8a1b9" :base0f "#b3886c")

;; ═════ Gates — escriba's convergence-layer invention ══════════════
(defgate :name "lsp-clean"
         :on-event "BufWritePost"
         :source "lsp.diagnostics"
         :severity "error"
         :action "warn"
         :message "Saved with unresolved LSP errors.")

(defgate :name "no-secrets"
         :on-event "BufWritePre"
         :source "secrets.scan"
         :action "reject"
         :message "Likely secret detected — write blocked.")

(defgate :name "rust-format-drift"
         :on-event "BufWritePre"
         :filetype "rust"
         :source "formatter.drift"
         :action "auto-fix"
         :message "Auto-formatted on save.")

(defgate :name "ts-parse-ok"
         :on-event "BufWritePre"
         :source "ts.query"
         :action "warn"
         :message "Tree-sitter parse errors in buffer.")

;; ═════ Workflows — escriba's editor-layer DAG invention ═══════════
(defworkflow :name "rust-ship"
             :description "Format drift check, cargo test, git push"
             :steps ("gate:rust-format-drift"
                     "shell:cargo test"
                     "gate:lsp-clean"
                     "action:git.push")
             :on-failure "abort"
             :keybind "<leader>ws")

(defworkflow :name "rust-commit"
             :description "Format-drift + no-secrets gate, then commit"
             :steps ("gate:rust-format-drift"
                     "gate:no-secrets"
                     "action:git.commit")
             :on-failure "prompt"
             :keybind "<leader>wc")

(defworkflow :name "save-and-test"
             :description "Write all buffers, run test suite"
             :steps ("cmd:write-all"
                     "shell:cargo test")
             :on-failure "continue"
             :keybind "<leader>wt")

;; ═════ Sessions — named workspace layouts ════════════════════════
(defsession :name "escriba-dev"
            :description "Working on escriba-lisp authoring bridge"
            :buffers ("escriba-lisp/src/lib.rs"
                      "escriba-lisp/src/apply.rs"
                      "escriba/configs/blnvim-defaults.lisp")
            :layout "horizontal"
            :cwd "~/code/github/pleme-io/escriba"
            :keybind "<leader>Se")

;; ═════ Effects — ghostty-style GPU shader layer ══════════════════
(defeffect :name "cursor-glow"
           :kind "cursor"
           :enable #t
           :intensity 0.6
           :radius 1.8
           :color "#94bbb8")

(defeffect :name "bloom"
           :kind "screen"
           :enable #t
           :intensity 0.25
           :threshold 0.75)

(defeffect :name "scanlines"
           :kind "screen"
           :enable #f
           :intensity 0.15)

;; ═════ Terms — typed bridge to mado over MCP ══════════════════════
(defterm :name "dev"
         :description "Default dev shell — frost in a horizontal split"
         :shell "/etc/profiles/per-user/drzzln/bin/frost"
         :placement "split-horizontal"
         :effects ("cursor-glow" "bloom")
         :keybind "<leader>td")

(defterm :name "watch"
         :description "cargo watch test — vertical split"
         :shell "cargo"
         :args ("watch" "-x" "test")
         :placement "split-vertical"
         :env ("CARGO_TERM_COLOR=always")
         :keybind "<leader>tw")

;; ═════ Marks — named positions ════════════════════════════════════
(defmark :name "'C"
         :description "escriba rc"
         :file "~/.config/escriba/rc.lisp"
         :line 1
         :kind "jump")

(defmark :name "'N"
         :description "nix flake.nix"
         :file "~/code/github/pleme-io/nix/flake.nix"
         :line 1
         :kind "jump")

;; ═════ Hash-referenced snippet (escriba-unique demo) ══════════════
(defsnippet :trigger "deploy-cmd"
            :hash "af42c0d18e9b3f4aa18b7c3ef1de93a4"
            :description "Team deploy command — content from mado store"
            :filetype "sh")

;; ═════ Schedules — typed declarative triggers (escriba-unique) ════
(defschedule :name "kick-format-buffer"
             :description "format the active buffer (bypass save hook)"
             :command "format-buffer"
             :keybind "<leader>kf")

;; ═════ Kmacros — declarative keyboard macros ══════════════════════
(defkmacro :name "insert-iso-date"
           :description "insert today's date on a new line"
           :keys ":put =strftime('%Y-%m-%d')<CR>"
           :mode "normal"
           :keybind "<leader>md")

(defkmacro :name "escape-insert"
           :description "classic jk -> Esc from insert mode"
           :keys "<Esc>"
           :mode "insert"
           :register "j")

;; ═════ Attestations — content-addressed rc integrity ═════════════
(defattest :id "blnvim-defaults-baseline"
           :description "pin this rc's shape once you're happy with it"
           :kind "pin"
           :severity "warn")

;; ═════ Rulers — vertical column guides ════════════════════════════
(defruler :columns (80 120)
          :style "soft"
          :color "#90897b"
          :description "classic 80 / 120 guides")

(defruler :columns (100)
          :filetype "rust"
          :style "soft"
          :color "#90897b"
          :description "rust line cap — clippy default")

;; ═════ Start screen ═══════════════════════════════════════════════
;; What escriba shows when it opens with no file — vim's `:intro`,
;; alpha.nvim's dashboard, the vscode welcome tab. The whole screen is
;; data: wordmark, tagline, menu. The renderers centre it and colour it
;; by ROLE (`escriba_ui::splash`), so it tracks the theme for free and
;; all three faces (tui / gpu / text) lay it out identically.
;;
;; Every entry's `:action` goes through the SAME resolver `defkeybind`
;; uses, so what an entry does is exactly what the equivalent keybind
;; would do — and the menu below lists only actions that are WIRED
;; today, not aspirational ones. A pressed key that did nothing would
;; be worse than a shorter menu.
;;
;; Any key that is not a menu key simply dismisses the screen and is
;; then handled normally, so the first thing you type is never eaten.
;; `:disabled #t` retires the screen without deleting the declaration.
(defsplash
  :tagline "Rust owns the invariants  ·  Lisp owns the authoring"
  :art ("███████╗███████╗ ██████╗██████╗ ██╗██████╗  █████╗ "
        "██╔════╝██╔════╝██╔════╝██╔══██╗██║██╔══██╗██╔══██╗"
        "█████╗  ███████╗██║     ██████╔╝██║██████╔╝███████║"
        "██╔══╝  ╚════██║██║     ██╔══██╗██║██╔══██╗██╔══██║"
        "███████╗███████║╚██████╗██║  ██║██║██████╔╝██║  ██║"
        "╚══════╝╚══════╝ ╚═════╝╚═╝  ╚═╝╚═╝╚═════╝ ╚═╝  ╚═╝")
  :entries ((:key "e" :label "start editing"     :action "normal")
            (:key "i" :label "insert text"       :action "insert")
            (:key "/" :label "search"            :action "search-forward")
            (:key ":" :label "command line"      :action "command")
            (:key "q" :label "quit"              :action "quit")))
