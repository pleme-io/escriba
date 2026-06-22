;; escriba-lspsaga — lightweight LSP UI (outline, peek, call hierarchy).
;; Mirrors nvimdev/lspsaga.nvim.
(defescribaplugin
  :name          "escriba-lspsaga"
  :version       "0.1.0"
  :category      "lsp"
  :description   "Lightweight LSP UI — outline, peek definition, call hierarchy"
  :blnvim-origin "nvimdev/lspsaga.nvim"
  :ativar-em     ("Event: LspAttach"))

(defhook :event "CursorMoved" :command "lsp.hover-preview-soft")

(defkeybind :mode "normal" :key "<leader>o"  :action "lsp.outline"        :description "toggle symbol outline")
(defkeybind :mode "normal" :key "<leader>ci" :action "lsp.incoming-calls" :description "incoming calls")
(defkeybind :mode "normal" :key "<leader>co" :action "lsp.outgoing-calls" :description "outgoing calls")
(defkeybind :mode "normal" :key "<leader>lp" :action "lsp.peek-definition" :description "peek definition")
(defcmd :name "Lspsaga" :description "open the lspsaga UI" :action "lsp.outline")
