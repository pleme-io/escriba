;; escriba-treesitter-fold — declarative per-filetype folding rules.
;; Absorbs vim foldmethod, nvim-treesitter-fold, vscode
;; FoldingRangeProvider into typed rc forms.
(defescribaplugin
  :name          "escriba-treesitter-fold"
  :version       "0.1.0"
  :category      "treesitter"
  :description   "Declarative per-filetype folding (treesitter / indent / marker / heading)"
  :blnvim-origin "nvim-treesitter (folds)"
  :ativar-em     ("Startup"))

(defkeybind :mode "normal" :key "za" :action "fold.toggle" :description "toggle fold")
(defkeybind :mode "normal" :key "zR" :action "fold.open-all" :description "open all folds")
(defkeybind :mode "normal" :key "zM" :action "fold.close-all" :description "close all folds")

(deffold :filetype "rust"
         :method "treesitter"
         :queries ("(function_item) @fold"
                   "(impl_item) @fold"
                   "(struct_item) @fold"
                   "(enum_item) @fold"
                   "(mod_item) @fold"
                   "(trait_item) @fold")
         :default-level 1)

(deffold :filetype "python"
         :method "indent"
         :trigger-chars "def class if for while"
         :default-level 1)

(deffold :filetype "markdown"
         :method "heading"
         :default-level 2)

(deffold :filetype "vim"
         :method "marker"
         :marker-start "{{{"
         :marker-end "}}}")

(deffold :filetype "typescript"
         :method "treesitter"
         :queries ("(function_declaration) @fold"
                   "(class_declaration) @fold"
                   "(interface_declaration) @fold")
         :default-level 1)
