;; escriba-git-conflict — merge-conflict navigation + resolution.
;; Mirrors akinsho/git-conflict.nvim.
(defescribaplugin
  :name          "escriba-git-conflict"
  :version       "0.1.0"
  :category      "git"
  :description   "Highlight + navigate + resolve git merge conflicts"
  :blnvim-origin "akinsho/git-conflict.nvim"
  :ativar-em     ("Event: BufReadPost"))

;; Keys namespaced under <leader>gc… (git-conflict) so they don't collide
;; with lspsaga's <leader>co / <leader>ci call-hierarchy bindings.
(defkeybind :mode "normal" :key "]x"          :action "conflict.next"          :description "next conflict")
(defkeybind :mode "normal" :key "[x"          :action "conflict.prev"          :description "previous conflict")
(defkeybind :mode "normal" :key "<leader>gco" :action "conflict.choose-ours"   :description "choose ours")
(defkeybind :mode "normal" :key "<leader>gct" :action "conflict.choose-theirs" :description "choose theirs")
(defkeybind :mode "normal" :key "<leader>gcb" :action "conflict.choose-both"   :description "choose both")

(defhighlight :group "GitConflictCurrent"  :bg "#4d543e")
(defhighlight :group "GitConflictIncoming" :bg "#2b3a4a")
