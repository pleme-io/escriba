;; escriba-ts-context-commentstring — AST-aware comment syntax.
;; Mirrors JoosepAlviste/nvim-ts-context-commentstring: pick the right
;; comment string from the tree-sitter node at point (e.g. {/* */} inside
;; JSX, // inside a <script>). Extends escriba-treesitter + escriba-comment.
(defescribaplugin
  :name          "escriba-ts-context-commentstring"
  :version       "0.1.0"
  :category      "treesitter"
  :description   "Context-aware comment string from the tree-sitter node at point"
  :blnvim-origin "JoosepAlviste/nvim-ts-context-commentstring"
  :ativar-em     ("Event: BufReadPost"))

(defoption :name "comment.context-aware" :value "true")
(defcmd :name "TSContextCommentstringToggle"
        :description "toggle AST-aware comment syntax"
        :action "comment.toggle-context-aware")
