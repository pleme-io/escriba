;; escriba-ts-autotag — auto-close + rename HTML / JSX tags via treesitter.
;; Mirrors windwp/nvim-ts-autotag. Extends the base escriba-treesitter
;; caixa.
(defescribaplugin
  :name          "escriba-ts-autotag"
  :version       "0.1.0"
  :category      "treesitter"
  :description   "Auto-close + auto-rename HTML / JSX tags via tree-sitter"
  :blnvim-origin "windwp/nvim-ts-autotag"
  :ativar-em     ("Event: InsertEnter"))

(defoption :name "ts-autotag.enabled"     :value "true")
(defoption :name "ts-autotag.auto-rename" :value "true")
(defcmd :name "TSAutotagToggle" :description "toggle HTML/JSX tag auto-close" :action "ts-autotag.toggle")
