;; escriba-fugitive — git command surface (status / diff / commit).
;; Mirrors tpope/vim-fugitive.
(defescribaplugin
  :name          "escriba-fugitive"
  :version       "0.1.0"
  :category      "git"
  :description   "Git command surface — status, diff, commit, blame"
  :blnvim-origin "tpope/vim-fugitive"
  :ativar-em     ("Command: Git"))

(defkeybind :mode "normal" :key "<leader>gs" :action "git.status" :description "git status")
(defkeybind :mode "normal" :key "<leader>gd" :action "git.diff"   :description "git diff")
(defkeybind :mode "normal" :key "<leader>gc" :action "git.commit" :description "git commit")
(defcmd :name "Git"  :description "run a git command"      :action "git.status")
(defcmd :name "Gdiff" :description "diff the active file"  :action "git.diff")
