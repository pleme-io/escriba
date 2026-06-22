;; escriba-tree — file-tree sidebar.
;; Mirrors nvim-tree/nvim-tree.lua.
(defescribaplugin
  :name          "escriba-tree"
  :version       "0.1.0"
  :category      "files"
  :description   "File-tree sidebar with git status + icons"
  :blnvim-origin "nvim-tree/nvim-tree.lua"
  :ativar-em     ("Command: NvimTreeToggle"))

(defkeybind :mode "normal" :key "<leader>fe" :action "tree.toggle" :description "toggle file tree")
(defcmd :name "NvimTreeToggle" :description "toggle the file-tree sidebar" :action "tree.toggle")
(defcmd :name "NvimTreeFindFile" :description "reveal the active file in the tree" :action "tree.find-file")
