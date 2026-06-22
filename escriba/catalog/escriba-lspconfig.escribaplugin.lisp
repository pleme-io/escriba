;; escriba-lspconfig — LSP client + curated server set.
;; Mirrors neovim/nvim-lspconfig + mason-lspconfig's default server set.
;; The entry below is plain escriba-lisp; escriba loads + applies it.
(defescribaplugin
  :name          "escriba-lspconfig"
  :version       "0.1.0"
  :category      "lsp"
  :description   "LSP client + curated language-server set (rust, ts, py, go, nix, …)"
  :blnvim-origin "neovim/nvim-lspconfig"
  :ativar-em     ("Startup"))

;; ── Navigation + actions (lsp keybinds) ──────────────────────────────
(defkeybind :mode "normal" :key "gd"         :action "lsp.definition"      :description "goto definition")
(defkeybind :mode "normal" :key "gr"         :action "lsp.references"      :description "find references")
(defkeybind :mode "normal" :key "gi"         :action "lsp.implementation"  :description "goto implementation")
(defkeybind :mode "normal" :key "gt"         :action "lsp.type-definition" :description "goto type definition")
(defkeybind :mode "normal" :key "K"          :action "lsp.hover"           :description "hover docs")
(defkeybind :mode "normal" :key "<leader>la" :action "lsp.code-action"     :description "code actions")
(defkeybind :mode "normal" :key "<leader>lr" :action "lsp.rename"          :description "rename symbol")
(defkeybind :mode "normal" :key "<leader>ld" :action "lsp.definition"      :description "goto definition")
(defkeybind :mode "normal" :key "<leader>lh" :action "lsp.hover"           :description "hover docs")
(defkeybind :mode "normal" :key "[d"         :action "lsp.diagnostic-prev" :description "previous diagnostic")
(defkeybind :mode "normal" :key "]d"         :action "lsp.diagnostic-next" :description "next diagnostic")

(defcmd :name "LspInfo"    :description "show attached language servers"  :action "lsp.info")
(defcmd :name "LspRestart" :description "restart the active server"       :action "lsp.restart")

;; ── Curated server set (mason-lspconfig default + escriba additions) ──
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
