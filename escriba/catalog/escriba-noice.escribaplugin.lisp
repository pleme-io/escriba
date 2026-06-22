;; escriba-noice — replace messages, cmdline, popupmenu UI.
;; Mirrors folke/noice.nvim.
(defescribaplugin
  :name          "escriba-noice"
  :version       "0.1.0"
  :category      "theming"
  :description   "Replace UI for messages, command line, and popup menu"
  :blnvim-origin "folke/noice.nvim"
  :ativar-em     ("Startup"))

(defoption :name "noice.enabled" :value "true")
(defcmd :name "NoiceDismiss" :description "dismiss visible notifications" :action "noice.dismiss")

(defhighlight :group "NoiceCmdline"       :fg "#e2dbc8" :bg "#1f1c15")
(defhighlight :group "NoiceCmdlinePopup"  :bg "#1f1c15")
