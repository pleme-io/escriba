;; escriba-luasnip — snippet engine + a starter snippet set.
;; Mirrors L3MON4D3/LuaSnip + rafamadriz/friendly-snippets.
(defescribaplugin
  :name          "escriba-luasnip"
  :version       "0.1.0"
  :category      "completion"
  :description   "Snippet engine with a starter library + jump navigation"
  :blnvim-origin "L3MON4D3/LuaSnip"
  :ativar-em     ("Event: InsertEnter"))

;; jump-prev is `<C-b>` and NOT LuaSnip's `<C-h>`, deliberately.
;;
;; `<C-h>` is backspace — terminals send 0x08 for it and vim treats the two
;; as one key in Insert. A LuaSnip user who takes `<C-h>` for snippets makes
;; that trade knowingly; escriba ships this catalog to EVERYONE by default,
;; and the snippet engine is not wired yet, so the binding traded a working
;; erase key for a dead one. Verified live before the move: typing `ZZZ` then
;; `<C-h>` erased nothing.
;;
;; `escriba/tests/insert_erase_survives_defaults.rs` now fails the build if
;; any bundled caixa shadows a core erase verb again.
(defkeybind :mode "insert" :key "<C-l>" :action "snippet.jump-next" :description "jump to next placeholder")
(defkeybind :mode "insert" :key "<C-b>" :action "snippet.jump-prev" :description "jump to previous placeholder")

(defsnippet :trigger "fn"     :filetype "rust" :body "fn ${1:name}(${2}) -> ${3:()} {\n    ${0}\n}")
(defsnippet :trigger "for"    :filetype "rust" :body "for ${1:item} in ${2:iter} {\n    ${0}\n}")
(defsnippet :trigger "if"     :filetype "rust" :body "if ${1:cond} {\n    ${0}\n}")
(defsnippet :trigger "struct" :filetype "rust" :body "struct ${1:Name} {\n    ${0}\n}")
(defsnippet :trigger "test"   :filetype "rust" :body "#[test]\nfn ${1:name}() {\n    ${0}\n}")
