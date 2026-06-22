;; escriba-luasnip — snippet engine + a starter snippet set.
;; Mirrors L3MON4D3/LuaSnip + rafamadriz/friendly-snippets.
(defescribaplugin
  :name          "escriba-luasnip"
  :version       "0.1.0"
  :category      "completion"
  :description   "Snippet engine with a starter library + jump navigation"
  :blnvim-origin "L3MON4D3/LuaSnip"
  :ativar-em     ("Event: InsertEnter"))

(defkeybind :mode "insert" :key "<C-l>" :action "snippet.jump-next" :description "jump to next placeholder")
(defkeybind :mode "insert" :key "<C-h>" :action "snippet.jump-prev" :description "jump to previous placeholder")

(defsnippet :trigger "fn"     :filetype "rust" :body "fn ${1:name}(${2}) -> ${3:()} {\n    ${0}\n}")
(defsnippet :trigger "for"    :filetype "rust" :body "for ${1:item} in ${2:iter} {\n    ${0}\n}")
(defsnippet :trigger "if"     :filetype "rust" :body "if ${1:cond} {\n    ${0}\n}")
(defsnippet :trigger "struct" :filetype "rust" :body "struct ${1:Name} {\n    ${0}\n}")
(defsnippet :trigger "test"   :filetype "rust" :body "#[test]\nfn ${1:name}() {\n    ${0}\n}")
