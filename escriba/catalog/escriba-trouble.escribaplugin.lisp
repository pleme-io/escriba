;; escriba-trouble — diagnostics / quickfix / loclist / refs panel.
;; Mirrors folke/trouble.nvim.
(defescribaplugin
  :name          "escriba-trouble"
  :version       "0.1.0"
  :category      "lsp"
  :description   "Pretty diagnostics / quickfix / loclist / LSP-references panel"
  :blnvim-origin "folke/trouble.nvim"
  :ativar-em     ("Command: Trouble"))

(defkeybind :mode "normal" :key "<leader>xx" :action "trouble.toggle"    :description "toggle diagnostics panel")
(defkeybind :mode "normal" :key "<leader>xw" :action "trouble.workspace" :description "workspace diagnostics")
(defkeybind :mode "normal" :key "<leader>xd" :action "trouble.document"  :description "document diagnostics")
(defcmd :name "Trouble" :description "toggle the trouble diagnostics panel" :action "trouble.toggle")
