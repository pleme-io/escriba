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
;; `<C-n>`, NOT `<C-Space>`.
;;
;; nvim-cmp's convention is `<C-Space>`, and escriba shipped that — but on
;; darwin the OS takes ctrl+space for application / input-source switching,
;; so the event never reaches the editor. The binding was not merely unwired
;; (cmp.complete is still inert); it was UNREACHABLE, and would have stayed
;; unreachable after being wired, presenting as "completion is broken".
;;
;; Caught by `escriba/tests/reserved_chords.rs` on its first run, against
;; `awase::Reserved::fleet_darwin()`. `<C-n>` is vim's own native completion
;; trigger and Insert mode binds nothing but Esc.
(defkeybind :mode "insert" :key "<C-n>"     :action "cmp.complete" :description "trigger completion")
(defkeybind :mode "insert" :key "<C-e>"     :action "cmp.abort"    :description "abort completion")
(defcmd :name "CmpToggle" :description "toggle autocompletion" :action "cmp.toggle")
