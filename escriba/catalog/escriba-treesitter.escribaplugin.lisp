;; escriba-treesitter — core tree-sitter parser + highlight engine.
;; Mirrors nvim-treesitter/nvim-treesitter (the base plugin the
;; -textobjects / -fold / -context / render-markdown caixas extend).
;; The parse engine itself lives in the escriba-ts crate + the baseline
;; `(defmode … :tree-sitter …)` registry; this caixa is its config home
;; and the explicit parity anchor.
(defescribaplugin
  :name          "escriba-treesitter"
  :version       "0.1.0"
  :category      "treesitter"
  :description   "Core tree-sitter parsing + syntax highlighting engine + config"
  :blnvim-origin "nvim-treesitter/nvim-treesitter"
  :ativar-em     ("Startup")
  :priority      800)

(defoption :name "treesitter.highlight"             :value "true")
(defoption :name "treesitter.indent"                :value "true")
(defoption :name "treesitter.incremental-selection" :value "true")
(defoption :name "treesitter.auto-install"          :value "true")

(defcmd :name "TSInstall"    :description "install a tree-sitter parser"       :action "treesitter.install")
(defcmd :name "TSUpdate"     :description "update installed tree-sitter parsers" :action "treesitter.update")
(defcmd :name "TSPlayground" :description "open the tree-sitter playground"     :action "treesitter.playground")
