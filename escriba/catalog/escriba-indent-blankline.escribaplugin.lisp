;; escriba-indent-blankline — indent guide lines.
;; Mirrors lukas-reineke/indent-blankline.nvim.
(defescribaplugin
  :name          "escriba-indent-blankline"
  :version       "0.1.0"
  :category      "theming"
  :description   "Indent guide lines + current-scope highlight"
  :blnvim-origin "lukas-reineke/indent-blankline.nvim"
  :ativar-em     ("Event: BufReadPost"))

(defoption :name "indent.guides"  :value "true")
(defcmd :name "IndentBlanklineToggle" :description "toggle indent guides" :action "indent.toggle")

(defhighlight :group "IndentBlanklineChar"        :fg "#2b2820")
(defhighlight :group "IndentBlanklineContextChar" :fg "#90897b")
