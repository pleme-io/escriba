;; escriba-comment — gcc / gc-motion comment toggling.
;; Mirrors numToStr/Comment.nvim.
(defescribaplugin
  :name          "escriba-comment"
  :version       "0.1.0"
  :category      "common"
  :description   "gcc / gc-motion line + block comment toggling"
  :blnvim-origin "numToStr/Comment.nvim"
  :ativar-em     ("Startup"))

(defkeybind :mode "normal" :key "gcc" :action "comment.toggle-line"  :description "toggle line comment")
(defkeybind :mode "normal" :key "gbc" :action "comment.toggle-block" :description "toggle block comment")
(defkeybind :mode "visual" :key "gc"  :action "comment.toggle-line"  :description "toggle comment on selection")
(defcmd :name "CommentToggle" :description "toggle comment on the current line" :action "comment.toggle-line")
