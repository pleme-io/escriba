;; escriba-lspkind — glyphs + labels for completion entries.
;; Mirrors onsails/lspkind.nvim.
(defescribaplugin
  :name          "escriba-lspkind"
  :version       "0.1.0"
  :category      "completion"
  :description   "Glyphs + labels for completion-menu entries"
  :blnvim-origin "onsails/lspkind.nvim"
  :ativar-em     ("Event: InsertEnter"))

(defoption :name "lspkind.mode"        :value "symbol_text")
(defoption :name "lspkind.max-width"   :value "50")
