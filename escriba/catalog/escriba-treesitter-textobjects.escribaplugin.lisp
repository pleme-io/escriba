;; escriba-treesitter-textobjects — tree-sitter text objects.
;; Mirrors nvim-treesitter/nvim-treesitter-textobjects: bind a TS query
;; to a vim i/a grammar name so users get vif / daf / cic across
;; filetypes. f=function c=class a=argument l=loop i=conditional
;; o=comment s=sexp — the vim ecosystem convention.
(defescribaplugin
  :name          "escriba-treesitter-textobjects"
  :version       "0.1.0"
  :category      "treesitter"
  :description   "Tree-sitter text objects — vif/daf/cic across filetypes"
  :blnvim-origin "nvim-treesitter/nvim-treesitter-textobjects"
  :ativar-em     ("Startup"))

;; ── Rust ─────────────────────────────────────────────────────────────
(deftextobject :name "f" :scope "outer" :filetype "rust"
               :query "(function_item) @function.outer")
(deftextobject :name "f" :scope "inner" :filetype "rust"
               :query "(function_item body: (block) @function.inner)")
(deftextobject :name "c" :scope "outer" :filetype "rust"
               :query "[(impl_item) (struct_item) (enum_item) (trait_item)] @class.outer")
(deftextobject :name "a" :scope "outer" :filetype "rust"
               :query "(parameter) @argument.outer")
(deftextobject :name "l" :scope "outer" :filetype "rust"
               :query "[(for_expression) (while_expression) (loop_expression)] @loop.outer")
(deftextobject :name "i" :scope "outer" :filetype "rust"
               :query "[(if_expression) (match_expression)] @conditional.outer")
(deftextobject :name "o" :scope "outer" :filetype "rust"
               :query "(line_comment) @comment.outer")

;; ── Python ───────────────────────────────────────────────────────────
(deftextobject :name "f" :scope "outer" :filetype "python"
               :query "(function_definition) @function.outer")
(deftextobject :name "f" :scope "inner" :filetype "python"
               :query "(function_definition body: (block) @function.inner)")
(deftextobject :name "c" :scope "outer" :filetype "python"
               :query "(class_definition) @class.outer")
(deftextobject :name "a" :scope "outer" :filetype "python"
               :query "(parameters (identifier) @argument.outer)")

;; ── Go ───────────────────────────────────────────────────────────────
(deftextobject :name "f" :scope "outer" :filetype "go"
               :query "(function_declaration) @function.outer")
(deftextobject :name "f" :scope "inner" :filetype "go"
               :query "(function_declaration body: (block) @function.inner)")

;; ── TypeScript / JavaScript ──────────────────────────────────────────
(deftextobject :name "f" :scope "outer" :filetype "typescript"
               :query "[(function_declaration) (arrow_function) (method_definition)] @function.outer")
(deftextobject :name "c" :scope "outer" :filetype "typescript"
               :query "(class_declaration) @class.outer")

;; ── Lisp (structural) ────────────────────────────────────────────────
(deftextobject :name "f" :scope "outer" :filetype "lisp"
               :query "(list_lit) @function.outer")
(deftextobject :name "s" :scope "outer" :filetype "lisp"
               :query "(list_lit) @sexp.outer"
               :description "sexp outer (paredit parity)")
