;; escriba — blnvim-compatible defaults
;; ----------------------------------------
;; This is the "batteries-included" configuration that reproduces the
;; out-of-the-box behavior of `pleme-io/blackmatter-nvim` (blnvim):
;; Nord theme, vim-style modal editing, 19 core plugins spanning the
;; same nine feature groups blnvim uses (common, completion,
;; formatting, keybindings, lsp, telescope, theming, tmux,
;; treesitter), plus escriba additions (files, git, ai).
;;
;; Load it explicitly:
;;
;;   escriba --rc escriba/configs/blnvim-defaults.lisp <file>
;;
;; Or copy into place as your personal rc:
;;
;;   cp escriba/configs/blnvim-defaults.lisp ~/.escribarc.lisp
;;
;; Everything is a re-declaration — no file inclusion, no magic load
;; order beyond "first writer wins" for (defkeybind), "last writer
;; wins" for (deftheme), and topological priority for (defplugin).

;; ═════ Theme ══════════════════════════════════════════════════════
;; blnvim's default is shaunsingh/nord.nvim. Match that.
(deftheme :preset "nord")

;; ═════ Options ════════════════════════════════════════════════════
;; Mirror the `:set` commands every blnvim user gets implicitly via
;; common.lua defaults.
(defoption :name "number"          :value "true")
(defoption :name "relativenumber"  :value "true")
(defoption :name "cursorline"      :value "true")
(defoption :name "expandtab"       :value "true")
(defoption :name "tabstop"         :value "4")
(defoption :name "shiftwidth"      :value "4")
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

;; ═════ Keybindings ════════════════════════════════════════════════
;; The blnvim leader is SPACE — matches the plurality of the nvim
;; ecosystem. `<leader>` is resolved at dispatch time against the
;; `:leader` option.
(defoption :name "mapleader"       :value "<space>")
(defoption :name "maplocalleader"  :value "<space>")

;; Motion — vim hjkl + half-page scrolls. `<C-d>` / `<C-u>` center
;; the screen after scrolling, matching `cinnamon.nvim`-style UX.
(defkeybind :mode "normal" :key "<C-d>" :action "half-page-down"
            :description "half page down, centered")
(defkeybind :mode "normal" :key "<C-u>" :action "half-page-up"
            :description "half page up, centered")

;; Buffer management (<leader>b…)
(defkeybind :mode "normal" :key "<leader>bd" :action "buffer.delete"
            :description "close the current buffer")

;; File operations
(defkeybind :mode "normal" :key "<leader>w" :action "save"
            :description "write the active buffer")
(defkeybind :mode "normal" :key "<leader>q" :action "quit"
            :description "quit the editor")

;; Search (<leader>f…) — mirrors telescope's default binding surface.
(defkeybind :mode "normal" :key "<leader>ff" :action "picker.files"
            :description "find files")
(defkeybind :mode "normal" :key "<leader>fg" :action "picker.grep"
            :description "grep workspace")
(defkeybind :mode "normal" :key "<leader>fb" :action "picker.buffers"
            :description "buffer switcher")
(defkeybind :mode "normal" :key "<leader>fh" :action "picker.help"
            :description "help tags")

;; LSP (<leader>l…) — mirrors lspsaga defaults.
(defkeybind :mode "normal" :key "<leader>la" :action "lsp.code-action"
            :description "code actions")
(defkeybind :mode "normal" :key "<leader>lr" :action "lsp.rename"
            :description "rename symbol")
(defkeybind :mode "normal" :key "<leader>ld" :action "lsp.definition"
            :description "goto definition")
(defkeybind :mode "normal" :key "<leader>lh" :action "lsp.hover"
            :description "hover docs")
(defkeybind :mode "normal" :key "<leader>lf" :action "lsp.format"
            :description "format buffer")

;; Git (<leader>g…)
(defkeybind :mode "normal" :key "<leader>gs" :action "git.status"
            :description "git status")
(defkeybind :mode "normal" :key "<leader>gb" :action "git.blame"
            :description "git blame")
(defkeybind :mode "normal" :key "<leader>gd" :action "git.diff"
            :description "git diff")

;; Diagnostics (<leader>x…) — mirrors trouble.nvim defaults.
(defkeybind :mode "normal" :key "<leader>xx" :action "trouble.toggle"
            :description "toggle diagnostics panel")
(defkeybind :mode "normal" :key "<leader>xw" :action "trouble.workspace"
            :description "workspace diagnostics")

;; File explorer (<leader>e…) — oil.nvim-style edit-the-fs-as-buffer.
(defkeybind :mode "normal" :key "<leader>e" :action "files.open"
            :description "open file explorer")

;; Tmux navigation (<C-h/j/k/l>) — compass.nvim-style cross-pane.
(defkeybind :mode "normal" :key "<C-h>" :action "pane.left"
            :description "focus left pane / tmux")
(defkeybind :mode "normal" :key "<C-j>" :action "pane.down"
            :description "focus below pane / tmux")
(defkeybind :mode "normal" :key "<C-k>" :action "pane.up"
            :description "focus above pane / tmux")
(defkeybind :mode "normal" :key "<C-l>" :action "pane.right"
            :description "focus right pane / tmux")

;; ═════ Hooks ══════════════════════════════════════════════════════
;; blnvim's "format on save" via conform.nvim.
(defhook :event "BufWritePre" :command "lsp.format-if-enabled")
;; gitsigns-style refresh on buffer enter.
(defhook :event "BufEnter" :command "git.refresh-signs")
;; lspsaga-style cursor-line hold for hover preview.
(defhook :event "CursorMoved" :command "lsp.hover-preview-soft")

;; ═════ Major modes ════════════════════════════════════════════════
;; Parity with blnvim's treesitter + ftplugin pairs. Languages here
;; match the grammars blnvim ships by default.
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

;; ═════ Plugins — blnvim canonical set (19) + escriba additions ════
;; Each plugin is a Lisp declaration mirroring blnvim's
;; `plugins/{author}/{name}/default.nix`. These are activation
;; descriptors — the plugin runtime (`escriba-plugin`) instantiates
;; them in priority order at startup (or on their lazy trigger).

;; ── Group: common ─────────────────────────────────────────────────
(defplugin :name "which-key"
           :description "Popup that displays pending keybinding completions"
           :category "common"
           :priority 900)

(defplugin :name "todo-comments"
           :description "Highlight + search TODO/FIXME/NOTE/HACK markers"
           :category "common"
           :on-event "BufReadPost"
           :lazy #t)

(defplugin :name "comment"
           :description "gcc / gc-motion line + block comment toggling"
           :category "common"
           :keybinds ("gc" "gcc" "gb")
           :lazy #t)

(defplugin :name "oil"
           :description "Edit the filesystem like a buffer"
           :category "files"
           :on-command "Oil"
           :keybinds ("<leader>e")
           :lazy #t)

(defplugin :name "compass"
           :description "Cross-editor pane navigation (tmux, wezterm, kitty)"
           :category "tmux"
           :keybinds ("<C-h>" "<C-j>" "<C-k>" "<C-l>"))

;; ── Group: completion ─────────────────────────────────────────────
(defplugin :name "cmp"
           :description "Autocompletion engine — sources: LSP / buffer / path / snippet"
           :category "completion"
           :on-event "InsertEnter"
           :lazy #t)

(defplugin :name "lspkind"
           :description "Glyphs + labels for completion entries"
           :category "completion"
           :on-event "InsertEnter"
           :lazy #t)

;; ── Group: formatting ─────────────────────────────────────────────
(defplugin :name "conform"
           :description "Pluggable formatter runner — format-on-save"
           :category "formatting"
           :on-event "BufWritePre"
           :lazy #t)

;; ── Group: lsp ────────────────────────────────────────────────────
(defplugin :name "mason"
           :description "Install and manage LSP servers, linters, formatters"
           :category "lsp"
           :on-command "Mason"
           :lazy #t)

(defplugin :name "mason-lspconfig"
           :description "Bridge between mason and nvim-lspconfig"
           :category "lsp"
           :on-event "LspAttach"
           :lazy #t)

(defplugin :name "lspsaga"
           :description "Lightweight LSP UI — hover, code actions, rename, diagnostics"
           :category "lsp"
           :on-event "LspAttach"
           :lazy #t)

(defplugin :name "lsp-signature"
           :description "Inline signature help as you type"
           :category "lsp"
           :on-event "InsertEnter"
           :lazy #t)

(defplugin :name "trouble"
           :description "Pretty diagnostic / quickfix / loclist / LSP refs panel"
           :category "lsp"
           :on-command "Trouble"
           :keybinds ("<leader>xx" "<leader>xw")
           :lazy #t)

(defplugin :name "tiny-inline-diagnostic"
           :description "Compact inline virtual-text diagnostics"
           :category "lsp"
           :on-event "LspAttach"
           :lazy #t)

;; ── Group: telescope ──────────────────────────────────────────────
(defplugin :name "telescope"
           :description "Fuzzy finder for files / buffers / grep / symbols / LSP"
           :category "telescope"
           :on-command "Telescope"
           :keybinds ("<leader>ff" "<leader>fg" "<leader>fb" "<leader>fh")
           :lazy #t)

;; ── Group: theming ────────────────────────────────────────────────
(defplugin :name "nord"
           :description "Nord colorscheme — arctic, muted palette"
           :category "theming"
           :priority 1000)

(defplugin :name "lualine"
           :description "Fast + configurable status line"
           :category "theming")

(defplugin :name "bufferline"
           :description "Buffer / tab line with diagnostic integration"
           :category "theming")

(defplugin :name "noice"
           :description "Replace UI messages + cmdline + popupmenu"
           :category "theming")

(defplugin :name "snacks"
           :description "Collection of small QoL UI widgets (dashboard, zen, etc.)"
           :category "theming")

(defplugin :name "nvim-notify"
           :description "Pretty notification popups"
           :category "theming"
           :on-event "VimEnter"
           :lazy #t)

(defplugin :name "gitsigns"
           :description "Git gutter signs, blame, hunks"
           :category "git"
           :on-event "BufReadPost"
           :lazy #t)

(defplugin :name "indent-blankline"
           :description "Indent guides"
           :category "theming"
           :on-event "BufReadPost"
           :lazy #t)

(defplugin :name "colorizer"
           :description "Inline hex / rgb / hsl color swatches"
           :category "theming"
           :on-event "BufReadPost"
           :lazy #t)

;; ── Group: treesitter ─────────────────────────────────────────────
(defplugin :name "render-markdown"
           :description "Render markdown structure (headings, tables, lists) in place"
           :category "treesitter"
           :on-filetype "markdown"
           :lazy #t)

;; ── escriba additions (not in blnvim) ──────────────────────────────
;; AI inline assistance via MCP. Escriba-specific — blnvim offers
;; windsurf + codeium; escriba's equivalent uses the existing
;; escriba-mcp server crate.
(defplugin :name "mcp-assist"
           :description "Inline AI edits + chat via the MCP protocol"
           :category "ai"
           :on-event "InsertEnter"
           :keybinds ("<leader>ai" "<leader>ac")
           :lazy #t)

;; ═════ Highlights — nord-palette syntax overrides ═════════════════
;; shaunsingh/nord.nvim parity for the common syntax groups. The
;; `deftheme :preset "nord"` form above seeds the baseline palette;
;; these tune individual groups on top. Extend freely.
(defhighlight :group "Normal"     :fg "#d8dee9" :bg "#2e3440")
(defhighlight :group "Comment"    :fg "#4c566a" :italic #t)
(defhighlight :group "String"     :fg "#a3be8c")
(defhighlight :group "Number"     :fg "#b48ead")
(defhighlight :group "Boolean"    :fg "#b48ead")
(defhighlight :group "Function"   :fg "#88c0d0" :bold #t)
(defhighlight :group "Keyword"    :fg "#81a1c1" :italic #t)
(defhighlight :group "Statement"  :fg "#81a1c1")
(defhighlight :group "Operator"   :fg "#81a1c1")
(defhighlight :group "Type"       :fg "#8fbcbb")
(defhighlight :group "Identifier" :fg "#eceff4")
(defhighlight :group "Constant"   :fg "#5e81ac")
(defhighlight :group "Special"    :fg "#ebcb8b")

;; ── UI ────────────────────────────────────────────────────────────
(defhighlight :group "CursorLine"   :bg "#3b4252")
(defhighlight :group "LineNr"       :fg "#4c566a")
(defhighlight :group "SignColumn"   :bg "#2e3440")
(defhighlight :group "Visual"       :bg "#434c5e")
(defhighlight :group "Search"       :fg "#2e3440" :bg "#ebcb8b")
(defhighlight :group "IncSearch"    :fg "#2e3440" :bg "#d08770" :bold #t)
(defhighlight :group "MatchParen"   :fg "#d08770" :bold #t)
(defhighlight :group "StatusLine"   :fg "#d8dee9" :bg "#3b4252")
(defhighlight :group "StatusLineNC" :fg "#4c566a" :bg "#2e3440")
(defhighlight :group "Pmenu"        :fg "#d8dee9" :bg "#3b4252")
(defhighlight :group "PmenuSel"     :fg "#2e3440" :bg "#88c0d0" :bold #t)
(defhighlight :group "NormalFloat"  :bg "#3b4252")
(defhighlight :group "FloatBorder"  :fg "#5e81ac" :bg "#3b4252")

;; ── Diagnostics ───────────────────────────────────────────────────
(defhighlight :group "DiagnosticError" :fg "#bf616a" :bold #t)
(defhighlight :group "DiagnosticWarn"  :fg "#ebcb8b")
(defhighlight :group "DiagnosticInfo"  :fg "#88c0d0")
(defhighlight :group "DiagnosticHint"  :fg "#a3be8c")

;; ── Git (gitsigns.nvim parity) ────────────────────────────────────
(defhighlight :group "GitSignsAdd"    :fg "#a3be8c")
(defhighlight :group "GitSignsChange" :fg "#ebcb8b")
(defhighlight :group "GitSignsDelete" :fg "#bf616a")
(defhighlight :group "DiffAdd"        :bg "#2d3f38")
(defhighlight :group "DiffChange"     :bg "#3d3f2b")
(defhighlight :group "DiffDelete"     :bg "#3f2d2d")

;; ── Tree-sitter semantic overrides ────────────────────────────────
(defhighlight :group "@function.call" :link "Function")
(defhighlight :group "@variable"      :link "Identifier")
(defhighlight :group "@parameter"     :fg "#d8dee9" :italic #t)
(defhighlight :group "@comment.todo"  :fg "#ebcb8b" :bold #t)
(defhighlight :group "@comment.note"  :fg "#88c0d0" :bold #t)
(defhighlight :group "@comment.warning" :fg "#d08770" :bold #t)

;; ═════ Status line — lualine-style composition ════════════════════
;; Three-slot layout matches lualine's `sections.lualine_*` shape.
;; Segment providers are resolved against the runtime's segment
;; table — plugins register more at load time.
(defstatusline
  :left ((:segment "mode"   :highlight "StatusLineMode")
         (:segment "branch" :highlight "GitSignsAdd" :prefix " ")
         (:segment "file"   :highlight "StatusLine"  :prefix " "))
  :center ()
  :right ((:segment "diagnostics")
          (:segment "lsp")
          (:segment "filetype" :prefix " ")
          (:segment "position" :prefix " :")
          (:segment "time"     :format "%H:%M" :prefix " ")))
