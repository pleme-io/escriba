;; escriba-todo-comments — TODO / FIXME / NOTE highlighting + navigation.
;; Mirrors folke/todo-comments.nvim. Owns the TODO-keyword highlight
;; groups (migrated out of the baseline rc).
(defescribaplugin
  :name          "escriba-todo-comments"
  :version       "0.1.0"
  :category      "common"
  :description   "Highlight + navigate TODO / FIXME / NOTE / HACK markers"
  :blnvim-origin "folke/todo-comments.nvim"
  :ativar-em     ("Event: BufReadPost"))

(defkeybind :mode "normal" :key "]t" :action "todo.next" :description "next TODO comment")
(defkeybind :mode "normal" :key "[t" :action "todo.prev" :description "previous TODO comment")
(defcmd :name "TodoQuickfix" :description "list TODO comments in the quickfix" :action "todo.quickfix")

(defhighlight :group "@comment.todo"    :fg "#d7c489" :bold #t)
(defhighlight :group "@comment.note"    :fg "#94bbb8" :bold #t)
(defhighlight :group "@comment.warning" :fg "#cb9070" :bold #t)
