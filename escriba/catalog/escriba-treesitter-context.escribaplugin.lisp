;; escriba-treesitter-context — sticky context header for the current scope.
;; Mirrors nvim-treesitter/nvim-treesitter-context.
(defescribaplugin
  :name          "escriba-treesitter-context"
  :version       "0.1.0"
  :category      "treesitter"
  :description   "Sticky header showing the enclosing function / class scope"
  :blnvim-origin "nvim-treesitter/nvim-treesitter-context"
  :ativar-em     ("Event: BufReadPost"))

(defoption :name "treesitter-context.enabled"    :value "true")
(defoption :name "treesitter-context.max-lines"  :value "3")
(defcmd :name "TSContextToggle" :description "toggle the sticky context header" :action "ts-context.toggle")

(defhighlight :group "TreesitterContext" :bg "#1f1c15")
