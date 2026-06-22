;; escriba-cmp — autocompletion engine.
;; Mirrors hrsh7th/nvim-cmp — sources: LSP / buffer / path / snippet.
(defescribaplugin
  :name          "escriba-cmp"
  :version       "0.1.0"
  :category      "completion"
  :description   "Autocompletion engine — LSP / buffer / path / snippet sources"
  :blnvim-origin "hrsh7th/nvim-cmp"
  :ativar-em     ("Event: InsertEnter"))

(defoption :name "cmp.enabled" :value "true")
(defoption :name "cmp.sources" :value "lsp,buffer,path,snippet")
(defkeybind :mode "insert" :key "<C-Space>" :action "cmp.complete" :description "trigger completion")
(defkeybind :mode "insert" :key "<C-e>"     :action "cmp.abort"    :description "abort completion")
(defcmd :name "CmpToggle" :description "toggle autocompletion" :action "cmp.toggle")
