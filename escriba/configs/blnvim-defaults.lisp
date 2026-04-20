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

;; ═════ Buffer line — bufferline.nvim parity ═══════════════════════
(defbufferline
  :separator "│"
  :modified-indicator "●"
  :show-close-icons #t
  :show-diagnostics #t
  :max-name-length 20)

;; ═════ LSP servers — mason-lspconfig default set ══════════════════
;; Every server that's in `mason.nvim`'s curated set + the
;; language-specific defaults escriba ships with. Commands assume
;; the server is on $PATH (mason installs them into ~/.local/share/
;; nvim/mason; the escriba runtime prepends that path analogously).
(deflsp :name "rust-analyzer"
        :command "rust-analyzer"
        :filetypes ("rust")
        :root-markers ("Cargo.toml" "rust-project.json"))

(deflsp :name "typescript"
        :command "typescript-language-server"
        :args ("--stdio")
        :filetypes ("typescript" "javascript")
        :root-markers ("tsconfig.json" "package.json" "jsconfig.json"))

(deflsp :name "pyright"
        :command "pyright-langserver"
        :args ("--stdio")
        :filetypes ("python")
        :root-markers ("pyproject.toml" "setup.py" "requirements.txt"))

(deflsp :name "gopls"
        :command "gopls"
        :filetypes ("go")
        :root-markers ("go.mod" "go.work"))

(deflsp :name "lua-language-server"
        :command "lua-language-server"
        :filetypes ("lua")
        :root-markers (".luarc.json" ".luarc.jsonc" "stylua.toml"))

(deflsp :name "nil"
        :command "nil"
        :filetypes ("nix")
        :root-markers ("flake.nix" "default.nix"))

(deflsp :name "bash-language-server"
        :command "bash-language-server"
        :args ("start")
        :filetypes ("sh"))

(deflsp :name "yaml-language-server"
        :command "yaml-language-server"
        :args ("--stdio")
        :filetypes ("yaml"))

(deflsp :name "terraformls"
        :command "terraform-ls"
        :args ("serve")
        :filetypes ("terraform"))

(deflsp :name "taplo"
        :command "taplo"
        :args ("lsp" "stdio")
        :filetypes ("toml"))

(deflsp :name "marksman"
        :command "marksman"
        :args ("server")
        :filetypes ("markdown"))

;; ═════ Formatters — conform.nvim parity ═══════════════════════════
;; format-on-save is ON by default per `:manual-only` polarity.
(defformatter :filetype "rust"       :command "rustfmt"  :args ("--edition" "2024"))
(defformatter :filetype "python"     :command "ruff"     :args ("format" "-"))
(defformatter :filetype "typescript" :command "prettier" :args ("--stdin-filepath" "$FILE"))
(defformatter :filetype "javascript" :command "prettier" :args ("--stdin-filepath" "$FILE"))
(defformatter :filetype "lua"        :command "stylua"   :args ("-"))
(defformatter :filetype "nix"        :command "alejandra")
(defformatter :filetype "go"         :command "gofmt")
(defformatter :filetype "terraform"  :command "terraform" :args ("fmt" "-"))
(defformatter :filetype "yaml"       :command "prettier" :args ("--parser" "yaml"))
(defformatter :filetype "markdown"   :command "prettier" :args ("--parser" "markdown"))
(defformatter :filetype "sh"         :command "shfmt"    :args ("-i" "2"))

;; ═════ Palette — nord canonical values ════════════════════════════
;; Users can reference `:preset "nord"` (above) or name this palette
;; explicitly via a `defhighlight :fg "nord.base0d"`-style ref once
;; the runtime's palette resolver is wired.
(defpalette :name "nord"
            :base00 "#2e3440" :base01 "#3b4252" :base02 "#434c5e"
            :base03 "#4c566a" :base04 "#d8dee9" :base05 "#e5e9f0"
            :base06 "#eceff4" :base07 "#eceff4"
            :base08 "#bf616a" :base09 "#d08770" :base0a "#ebcb8b"
            :base0b "#a3be8c" :base0c "#8fbcbb" :base0d "#88c0d0"
            :base0e "#81a1c1" :base0f "#5e81ac")

;; ═════ Icons — nvim-web-devicons parity (canonical subset) ════════
(deficon :filetype "rust"       :glyph "" :fg "#dea584")
(deficon :filetype "python"     :glyph "" :fg "#ffbc03")
(deficon :filetype "javascript" :glyph "" :fg "#cbcb41")
(deficon :filetype "typescript" :glyph "" :fg "#519aba")
(deficon :filetype "go"         :glyph "" :fg "#519aba")
(deficon :filetype "lua"        :glyph "" :fg "#51a0cf")
(deficon :filetype "nix"        :glyph "" :fg "#7ebae4")
(deficon :filetype "lisp"       :glyph "" :fg "#87af5f")
(deficon :filetype "markdown"   :glyph "" :fg "#519aba")
(deficon :filetype "yaml"       :glyph "" :fg "#6d8086")
(deficon :filetype "toml"       :glyph "" :fg "#9c4221")
(deficon :filetype "json"       :glyph "" :fg "#cbcb41")
(deficon :filetype "sh"         :glyph "" :fg "#89e051")
(deficon :filetype "terraform"  :glyph "" :fg "#5f43e9")
;; Pattern-based icons — `Cargo.*`, `Makefile`, `.envrc` etc.
(deficon :pattern "Cargo.toml"   :glyph "" :fg "#dea584")
(deficon :pattern "Cargo.lock"   :glyph "" :fg "#dea584")
(deficon :pattern "flake.nix"    :glyph "" :fg "#7ebae4")
(deficon :pattern "flake.lock"   :glyph "" :fg "#7ebae4")
(deficon :pattern "package.json" :glyph "" :fg "#e8274b")
(deficon :pattern "Makefile"     :glyph "" :fg "#6d8086")
(deficon :pattern "Dockerfile"   :glyph "" :fg "#458ee6")
(deficon :pattern ".envrc"       :glyph "" :fg "#89e051")
(deficon :pattern ".gitignore"   :glyph "" :fg "#e24329")

;; ═════ DAP adapters — nvim-dap parity (common languages) ══════════
(defdap :name "lldb"
        :command "lldb-dap"
        :filetypes ("rust" "c" "cpp"))
(defdap :name "codelldb"
        :command "codelldb"
        :filetypes ("rust" "c" "cpp"))
(defdap :name "debugpy"
        :command "python"
        :args ("-m" "debugpy.adapter")
        :filetypes ("python"))
(defdap :name "delve"
        :command "dlv"
        :args ("dap" "-l" "127.0.0.1:38697")
        :filetypes ("go")
        :port 38697)
(defdap :name "js-debug"
        :command "node"
        :args ("-e" "require('@vscode/js-debug')")
        :filetypes ("typescript" "javascript"))

;; ═════ Gates — escriba's convergence-layer invention ══════════════
;; `defgate` is unique to escriba: a typed pre/post-condition on an
;; editor event, with action = reject / warn / auto-fix. No other
;; editor has this as a first-class declarative form. Mirrors the
;; tatara convergence-computing pattern (prepare → execute →
;; verify → attest) at the editor layer.
;;
;; `reject`  — hard-fail the event (write aborted, buffer stays dirty).
;; `warn`    — log + let the event proceed (diagnostic surface).
;; `auto-fix` — run `:auto-fix`, retry; falls back to reject on 2nd fail.

;; LSP: warn if writing while the buffer carries an error diagnostic.
(defgate :name "lsp-clean"
         :on-event "BufWritePost"
         :source "lsp.diagnostics"
         :severity "error"
         :action "warn"
         :message "Saved with unresolved LSP errors.")

;; Secrets: reject writes that look like they committed a secret.
(defgate :name "no-secrets"
         :on-event "BufWritePre"
         :source "secrets.scan"
         :action "reject"
         :message "Likely secret detected — write blocked.")

;; Rust formatter drift: auto-fix via rustfmt on save.
(defgate :name "rust-format-drift"
         :on-event "BufWritePre"
         :filetype "rust"
         :source "formatter.drift"
         :action "auto-fix"
         :message "Auto-formatted on save.")

;; Tree-sitter parse health: warn if the buffer doesn't parse cleanly.
(defgate :name "ts-parse-ok"
         :on-event "BufWritePre"
         :source "ts.query"
         :action "warn"
         :message "Tree-sitter parse errors in buffer.")

;; ═════ Text objects — nvim-treesitter-textobjects canonical set ═══
;; Each `deftextobject` binds a tree-sitter query to a vim i/a grammar
;; name, so users get `vif` / `daf` / `cic` etc. across filetypes
;; without per-plugin plumbing. `f` = function, `c` = class, `a` =
;; argument, `l` = loop, `i` = conditional, `o` = comment, `p` =
;; paragraph — the vim ecosystem convention.

;; ── Rust ──────────────────────────────────────────────────────────
(deftextobject :name "f" :scope "outer" :filetype "rust"
               :query "(function_item) @function.outer")
(deftextobject :name "f" :scope "inner" :filetype "rust"
               :query "(function_item body: (block) @function.inner)")
(deftextobject :name "c" :scope "outer" :filetype "rust"
               :query "[(impl_item) (struct_item) (enum_item) (trait_item)] @class.outer")
(deftextobject :name "a" :scope "outer" :filetype "rust"
               :query "(parameter) @argument.outer")
(deftextobject :name "l" :scope "outer" :filetype "rust"
               :query "[(for_expression) (while_expression) (loop_expression)] @loop.outer")
(deftextobject :name "i" :scope "outer" :filetype "rust"
               :query "[(if_expression) (match_expression)] @conditional.outer")
(deftextobject :name "o" :scope "outer" :filetype "rust"
               :query "(line_comment) @comment.outer")

;; ── Python ────────────────────────────────────────────────────────
(deftextobject :name "f" :scope "outer" :filetype "python"
               :query "(function_definition) @function.outer")
(deftextobject :name "f" :scope "inner" :filetype "python"
               :query "(function_definition body: (block) @function.inner)")
(deftextobject :name "c" :scope "outer" :filetype "python"
               :query "(class_definition) @class.outer")
(deftextobject :name "a" :scope "outer" :filetype "python"
               :query "(parameters (identifier) @argument.outer)")

;; ── Go ────────────────────────────────────────────────────────────
(deftextobject :name "f" :scope "outer" :filetype "go"
               :query "(function_declaration) @function.outer")
(deftextobject :name "f" :scope "inner" :filetype "go"
               :query "(function_declaration body: (block) @function.inner)")

;; ── TypeScript / JavaScript ───────────────────────────────────────
(deftextobject :name "f" :scope "outer" :filetype "typescript"
               :query "[(function_declaration) (arrow_function) (method_definition)] @function.outer")
(deftextobject :name "c" :scope "outer" :filetype "typescript"
               :query "(class_declaration) @class.outer")

;; ── Lisp (structural) ─────────────────────────────────────────────
(deftextobject :name "f" :scope "outer" :filetype "lisp"
               :query "(list_lit) @function.outer")
(deftextobject :name "s" :scope "outer" :filetype "lisp"
               :query "(list_lit) @sexp.outer"
               :description "sexp outer (paredit parity)")

;; ═════ Workflows — escriba's editor-layer DAG invention ═══════════
;; Named sequences of gates + actions the editor walks on demand.
;; Every step is `kind:ref` (`gate:…`, `action:…`, `workflow:…`,
;; `shell:…`, `cmd:…`). On-failure is `abort` (default) / `continue`
;; / `prompt`. Chains of convergence checks + side effects that the
;; runtime attests as it walks them — one tier deeper than defgate.

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
;; Absorbs vim :mksession, vscode workspaces, emacs desktop-save.
;; Each session is a named bundle of buffers + layout + cwd + hooks
;; the picker can activate. Escriba-specific: sessions are *intent*
;; declared in rc, not a serialized point-in-time snapshot.

(defsession :name "escriba-dev"
            :description "Working on escriba-lisp authoring bridge"
            :buffers ("escriba-lisp/src/lib.rs"
                      "escriba-lisp/src/apply.rs"
                      "escriba/configs/blnvim-defaults.lisp")
            :layout "horizontal"
            :cwd "~/code/github/pleme-io/escriba"
            :keybind "<leader>Se")

(defsession :name "frost-dev"
            :description "Working on frost shell"
            :buffers ("crates/frost/src/main.rs"
                      "crates/frost-lisp/src/lib.rs")
            :layout "horizontal"
            :cwd "~/code/github/pleme-io/frost"
            :keybind "<leader>Sf")

;; ═════ Effects — ghostty-style GPU shader layer ══════════════════
;; Applies to --render=gpu only; TUI mode ignores silently. Cursor
;; glow, bloom, scanlines, film grain — ghostty / mado parity. Users
;; opt in via :enable #t; defaults here record the preferred shape
;; without firing until the user flips it on.

(defeffect :name "cursor-glow"
           :kind "cursor"
           :enable #t
           :intensity 0.6
           :radius 1.8
           :color "#88c0d0")

(defeffect :name "cursor-trail"
           :kind "cursor-trail"
           :enable #f
           :intensity 0.4
           :color "#81a1c1")

(defeffect :name "bloom"
           :kind "screen"
           :enable #t
           :intensity 0.25
           :threshold 0.75)

(defeffect :name "scanlines"
           :kind "screen"
           :enable #f
           :intensity 0.15)

(defeffect :name "film-grain"
           :kind "screen"
           :enable #f
           :intensity 0.08)

(defeffect :name "underglow"
           :kind "underglow"
           :enable #f
           :intensity 0.5
           :color "#5e81ac")

;; ═════ Terms — typed bridge to mado over MCP ══════════════════════
;; Wire-compatible with mado's TermSpec. Activating a defterm sends
;; its payload to mado's `spawn_term` MCP tool — the same typed
;; contract both repos consume. Placement mirrors mado's enum,
;; effects reference this rc's defeffect names.

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

(defterm :name "side"
         :description "Throwaway shell in a new window"
         :placement "window"
         :keybind "<leader>tn")

;; ═════ Marks — named positions with jump/anchor/glance semantics ══
;; Absorbs vim global marks (`'A`..`'Z`) + emacs bookmarks. Escriba
;; extension: `:kind` picks navigation style.
;;   jump   — vim-style cursor move + jumplist push (default).
;;   anchor — jump AND pin the file in a side split so it stays visible.
;;   glance — zed-style peek without moving the primary cursor.

(defmark :name "'C"
         :description "escriba rc"
         :file "~/.config/escriba/rc.lisp"
         :line 1
         :kind "jump")

(defmark :name "'F"
         :description "frost rc (frostmourne)"
         :file "~/code/github/pleme-io/frostmourne/lisp/00-core.lisp"
         :line 1
         :kind "jump")

(defmark :name "'N"
         :description "nix flake.nix"
         :file "~/code/github/pleme-io/nix/flake.nix"
         :line 1
         :kind "jump")

;; ═════ Hash-referenced snippets (escriba-unique) ══════════════════
;; Escriba is the only editor that references snippet bodies by
;; BLAKE3 content hash. The hash format matches `mado::clipboard_store`
;; so a payload copied in the terminal ends up addressable from the
;; editor via the same token — no payload traverses the MCP socket.
;;
;; The example below references a hash that isn't pre-populated in
;; any store yet; it's documentation for the wire shape. Drop real
;; hashes in your own rc once you start using the content-store
;; (sample flow: copy payload in mado with OSC 52 → mado returns a
;; hash in its log / MCP → add a defsnippet referencing it).
(defsnippet :trigger "deploy-cmd"
            :hash "af42c0d18e9b3f4aa18b7c3ef1de93a4"
            :description "Team deploy command — content from mado store"
            :filetype "sh")

;; ═════ Tasks — runnable shell command per filetype ════════════════
;; Absorbs vscode tasks.json, nvim asynctasks, jetbrains run-configs,
;; emacs projectile-run-command. One shell invocation with filetype /
;; cwd / env scope. A task with a `:keybind` fires without opening a
;; picker; `:background #t` runs without blocking the editor and reports
;; completion via notification (OSC 9 in terminal mode).

(deftask :name "cargo-test"
         :description "cargo test --workspace for the current project"
         :command "cargo"
         :args ("test" "--workspace")
         :filetype "rust"
         :env ("CARGO_TERM_COLOR=always" "RUST_LOG=warn")
         :background #t
         :keybind "<leader>rt"
         :timeout-ms 600000)

(deftask :name "cargo-check"
         :description "cargo check for fast type pass"
         :command "cargo"
         :args ("check" "--workspace" "--all-targets")
         :filetype "rust"
         :background #t
         :keybind "<leader>rc"
         :timeout-ms 180000)

(deftask :name "cargo-run"
         :description "cargo run (primary binary)"
         :command "cargo"
         :args ("run")
         :filetype "rust"
         :keybind "<leader>rr")

(deftask :name "fleet-rebuild"
         :description "nix run .#rebuild — apply the pleme-io fleet"
         :command "nix"
         :args ("run" ".#rebuild")
         :cwd "~/code/github/pleme-io/nix"
         :env ("RUST_LOG=warn")
         :background #t
         :keybind "<leader>rR"
         :timeout-ms 1800000)

(deftask :name "rg-todos"
         :description "rg TODO/FIXME across the active cwd"
         :command "rg"
         :args ("-n" "--pretty" "TODO|FIXME|XXX|HACK" ".")
         :keybind "<leader>ft")

(deftask :name "escriba-doctor"
         :description "dump the parsed ApplyPlan summary"
         :command "escriba"
         :args ("--list-rc")
         :keybind "<leader>ed")

;; ═════ Schedules — typed declarative triggers (escriba-unique) ════
;; No editor in the category has this: emacs has `run-at-time`, nvim
;; has `vim.defer_fn`, vscode has `setInterval` — all untyped calls.
;; Escriba ships temporal triggers as a first-class def-form bound to
;; the typed command / workflow registry.
;;
;; Shape rules:
;;   * exactly one of :cron / :interval-seconds / :idle-seconds /
;;     :at-startup (or none, for manual-only via :keybind)
;;   * exactly one of :command / :workflow / :action as dispatch
;;
;; These samples are deliberately conservative. Uncomment when you
;; want a schedule to fire automatically.

;; (defschedule :name "autosave-on-idle"
;;              :description "save every modified buffer after 30s idle"
;;              :idle-seconds 30
;;              :command "save-all")

;; (defschedule :name "refresh-diagnostics-5min"
;;              :description "poll LSP + linters every five minutes"
;;              :interval-seconds 300
;;              :workflow "diagnostics-refresh")

;; (defschedule :name "top-of-hour-pull"
;;              :description "git pull on the hour when idle on main"
;;              :cron "0 * * * *"
;;              :command "git.pull")

;; Manual-only — reusing the schedule dispatch machinery for a
;; keybind that fires the workflow directly, without subscribing
;; to any automatic trigger.
(defschedule :name "kick-format-buffer"
             :description "format the active buffer (bypass save hook)"
             :command "format-buffer"
             :keybind "<leader>kf")

;; ═════ Kmacros — declarative keyboard macros ══════════════════════
;; Absorbs vim's q/Q recording + @ replay, emacs kmacro.el, jetbrains
;; keyboard macros. Escriba lifts the concept out of volatile
;; registers into typed, reviewable rc entries. Replay via `:keybind`
;; or classic `@<register>` when `:register` is set.

(defkmacro :name "insert-iso-date"
           :description "insert today's date on a new line"
           :keys ":put =strftime('%Y-%m-%d')<CR>"
           :mode "normal"
           :keybind "<leader>md")

(defkmacro :name "wrap-in-backticks"
           :description "wrap visual selection in markdown code span"
           :keys "c`<C-r>\"`<Esc>"
           :mode "visual"
           :filetype "markdown"
           :keybind "<leader>m`")

(defkmacro :name "escape-insert"
           :description "classic jk → Esc from insert mode"
           :keys "<Esc>"
           :mode "insert"
           :register "j")

;; ═════ Attestations — content-addressed rc integrity (escriba-unique) ══
;; No editor in the category signs its own config via content hash.
;; `defattest` pins an expected BLAKE3-128 hex of this rc's
;; `(ApplyPlan::content_summary)` — the shape of the plan minus the
;; attestation count itself (so adding an attestation doesn't
;; invalidate its own pin).
;;
;; Populate `:counts-hash` with the value `escriba --list-rc` prints
;; under the content-hash line once you're happy with the rc shape.
;; On every subsequent load, `escriba doctor` (planned) compares the
;; actual vs expected hash and escalates based on `:severity`.
;;
;; The stub below is intentionally unpinned — `evaluate_attests`
;; returns `Skipped` for an empty `:counts-hash`, so this compiles
;; cleanly without locking users out until they're ready.

(defattest :id "blnvim-defaults-baseline"
           :description "pin this rc's shape once you're happy with it"
           :kind "pin"
           :severity "warn")

;; ═════ Rulers — vertical column guides ════════════════════════════
;; Absorbs vim colorcolumn, vscode editor.rulers, jetbrains hard-wrap
;; margin. Declarative, filetype-scoped, typed.

(defruler :columns (80 120)
          :style "soft"
          :color "#4c566a"
          :description "classic 80 / 120 guides")

(defruler :columns (100)
          :filetype "rust"
          :style "soft"
          :color "#4c566a"
          :description "rust line cap — clippy default")

(defruler :columns (80)
          :filetype "markdown"
          :style "dim"
          :description "markdown soft wrap at 80")

;; ═════ MCP-tool bindings — escriba → mado / curupira / fleet ══════
;; Declarative `defmcp` forms turn any pleme-io MCP server's tools
;; into first-class escriba commands. No editor in the category
;; ships typed cross-process MCP import — vscode extensions are
;; JavaScript, zed is Rust-compiled, neovim's LSP client isn't
;; MCP-native. escriba resolves the typed binding at apply time
;; and surfaces the tool in the command palette with an optional
;; keybind + result dispatch.

;; Mado clipboard bridge — the full content-addressed lifecycle.
(defmcp :name "mado.clipboard.get"
        :description "fetch a BLAKE3-addressed clipboard payload from mado"
        :server "mado"
        :tool "clipboard_get"
        :keybind "<leader>mcg"
        :on-result "action:insert-at-cursor")

(defmcp :name "mado.clipboard.put"
        :description "publish the current selection into mado's store"
        :server "mado"
        :tool "clipboard_put"
        :keybind "<leader>mcp")

(defmcp :name "mado.clipboard.list"
        :description "list recent clipboard entries from mado"
        :server "mado"
        :tool "clipboard_list"
        :keybind "<leader>mcl")

(defmcp :name "mado.clipboard.clear"
        :description "scrub mado's clipboard history (sensitive content)"
        :server "mado"
        :tool "clipboard_clear"
        :keybind "<leader>mcC")

;; Mado prompt-jump bridge — new since the OSC 133 history tick.
(defmcp :name "mado.prompt.list"
        :description "list OSC 133 prompt marks across mado sessions"
        :server "mado"
        :tool "prompt_marks_list"
        :keybind "<leader>mpp")

(defmcp :name "mado.prompt.clear"
        :description "clear mado's prompt-mark history"
        :server "mado"
        :tool "prompt_marks_clear")

(defmcp :name "mado.attention.set"
        :description "flip mado's OSC 1337 RequestAttention flag"
        :server "mado"
        :tool "attention_set")

;; ═════ Workflows using mcp:<server>.<tool> step kind ═══════════════
;; Novel to escriba — no editor ships typed MCP-driven workflow
;; steps. Every `mcp:…` step is cross-validated at apply time
;; against the defmcp set so dangling tool references fail fast.

(defworkflow :name "ship-and-flash"
             :description "test, push, then flash mado's dock on success"
             :steps ("shell:cargo test"
                     "action:git.push"
                     "mcp:mado.attention_set")
             :on-failure "abort"
             :keybind "<leader>ws")

;; ═════ Folds — declarative per-filetype folding rules ══════════════
;; Absorbs vim foldmethod, nvim-treesitter-fold, vscode
;; FoldingRangeProvider. Every editor has folding, but the shape
;; varies: vim ships a scalar option, nvim puts it in a Lua plugin,
;; vscode registers a programmatic provider. deffold lifts the whole
;; axis into typed rc.
;;
;; Methods: treesitter | indent | marker | heading | syntax
;; (matches vim's foldmethod vocabulary).

(deffold :filetype "rust"
         :method "treesitter"
         :queries ("(function_item) @fold"
                   "(impl_item) @fold"
                   "(struct_item) @fold"
                   "(enum_item) @fold"
                   "(mod_item) @fold"
                   "(trait_item) @fold")
         :default-level 1)

(deffold :filetype "python"
         :method "indent"
         :trigger-chars "def class if for while"
         :default-level 1)

(deffold :filetype "markdown"
         :method "heading"
         :default-level 2)

(deffold :filetype "vim"
         :method "marker"
         :marker-start "{{{"
         :marker-end "}}}")

(deffold :filetype "typescript"
         :method "treesitter"
         :queries ("(function_declaration) @fold"
                   "(class_declaration) @fold"
                   "(interface_declaration) @fold")
         :default-level 1)
