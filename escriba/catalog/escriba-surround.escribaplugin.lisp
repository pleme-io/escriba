;; escriba-surround — add / change / delete surrounding pairs.
;; Mirrors kylechui/nvim-surround (vim-surround lineage).
(defescribaplugin
  :name          "escriba-surround"
  :version       "0.1.0"
  :category      "common"
  :description   "Add / change / delete surrounding pairs (quotes, brackets, tags)"
  :blnvim-origin "kylechui/nvim-surround"
  :ativar-em     ("Startup"))

(defoption :name "surround.enabled" :value "true")
(defkeybind :mode "normal" :key "ys" :action "surround.add"    :description "add surround (motion)")
(defkeybind :mode "normal" :key "ds" :action "surround.delete" :description "delete surround")
(defkeybind :mode "normal" :key "cs" :action "surround.change" :description "change surround")
(defkeybind :mode "visual" :key "S"  :action "surround.add"    :description "surround selection")
