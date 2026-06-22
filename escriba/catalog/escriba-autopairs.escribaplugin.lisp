;; escriba-autopairs — auto-close brackets / quotes.
;; Mirrors windwp/nvim-autopairs.
(defescribaplugin
  :name          "escriba-autopairs"
  :version       "0.1.0"
  :category      "common"
  :description   "Auto-close brackets, parens, and quotes as you type"
  :blnvim-origin "windwp/nvim-autopairs"
  :ativar-em     ("Event: InsertEnter"))

(defoption :name "autopairs.enabled"      :value "true")
(defoption :name "autopairs.close-on-cr"  :value "true")
(defcmd :name "AutopairsToggle" :description "toggle auto-pairing" :action "autopairs.toggle")
