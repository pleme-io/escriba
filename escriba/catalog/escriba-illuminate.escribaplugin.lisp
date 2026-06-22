;; escriba-illuminate — highlight other uses of the symbol under cursor.
;; Mirrors RRethy/vim-illuminate.
(defescribaplugin
  :name          "escriba-illuminate"
  :version       "0.1.0"
  :category      "lsp"
  :description   "Highlight all usages of the symbol under the cursor"
  :blnvim-origin "RRethy/vim-illuminate"
  :ativar-em     ("Event: LspAttach"))

(defoption :name "illuminate.enabled"   :value "true")
(defoption :name "illuminate.delay-ms"  :value "120")

(defkeybind :mode "normal" :key "]]" :action "illuminate.next" :description "next reference")
(defkeybind :mode "normal" :key "[[" :action "illuminate.prev" :description "previous reference")

(defhighlight :group "LspReferenceText"  :bg "#2b2820")
(defhighlight :group "LspReferenceRead"  :bg "#2b2820")
(defhighlight :group "LspReferenceWrite" :bg "#2b3a4a")
