;; escriba-conform — pluggable formatter runner (format-on-save).
;; Mirrors stevearc/conform.nvim — one formatter per filetype, run on
;; the BufWritePre hook and on demand via <leader>lf.
(defescribaplugin
  :name          "escriba-conform"
  :version       "0.1.0"
  :category      "formatting"
  :description   "Pluggable formatter runner — format on save + on demand"
  :blnvim-origin "stevearc/conform.nvim"
  :ativar-em     ("Startup"))

;; Format-on-save + on-demand format.
(defhook :event "BufWritePre" :command "lsp.format-if-enabled")
(defkeybind :mode "normal" :key "<leader>lf" :action "lsp.format" :description "format buffer")
(defcmd :name "Format" :description "format the active buffer" :action "lsp.format")

;; ── Formatters (conform.nvim parity) ─────────────────────────────────
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
