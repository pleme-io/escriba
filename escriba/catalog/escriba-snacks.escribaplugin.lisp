;; escriba-snacks — QoL UI widgets (dashboard, zen, terminal, gitbrowse).
;; Mirrors folke/snacks.nvim.
(defescribaplugin
  :name          "escriba-snacks"
  :version       "0.1.0"
  :category      "theming"
  :description   "QoL UI widgets — dashboard, zen mode, terminal, git-browse"
  :blnvim-origin "folke/snacks.nvim"
  :ativar-em     ("Startup"))

(defkeybind :mode "normal" :key "<leader>z"  :action "snacks.zen"       :description "toggle zen mode")
(defkeybind :mode "normal" :key "<leader>tt" :action "snacks.terminal"  :description "toggle terminal")
(defkeybind :mode "normal" :key "<leader>gg" :action "snacks.gitbrowse" :description "open in browser")
(defcmd :name "SnacksDashboard" :description "open the start dashboard" :action "snacks.dashboard")
(defcmd :name "SnacksZen"       :description "toggle zen mode"          :action "snacks.zen")
