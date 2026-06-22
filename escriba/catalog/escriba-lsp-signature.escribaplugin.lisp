;; escriba-lsp-signature — inline signature help as you type.
;; Mirrors ray-x/lsp_signature.nvim.
(defescribaplugin
  :name          "escriba-lsp-signature"
  :version       "0.1.0"
  :category      "lsp"
  :description   "Inline signature help while typing function arguments"
  :blnvim-origin "ray-x/lsp_signature.nvim"
  :ativar-em     ("Event: InsertEnter"))

(defoption :name "signature.enabled" :value "true")
(defkeybind :mode "insert" :key "<C-k>" :action "lsp.signature" :description "signature help")
(defcmd :name "LspSignature" :description "show signature help" :action "lsp.signature")
