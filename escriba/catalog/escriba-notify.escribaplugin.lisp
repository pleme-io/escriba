;; escriba-notify — pretty notification popups.
;; Mirrors rcarriga/nvim-notify.
(defescribaplugin
  :name          "escriba-notify"
  :version       "0.1.0"
  :category      "theming"
  :description   "Animated notification popups with history"
  :blnvim-origin "rcarriga/nvim-notify"
  :ativar-em     ("Event: VimEnter"))

(defoption :name "notify.enabled"   :value "true")
(defoption :name "notify.timeout-ms" :value "3000")
(defcmd :name "Notifications" :description "show notification history" :action "notify.history")

(defhighlight :group "NotifyERRORTitle" :fg "#c9837b" :bold #t)
(defhighlight :group "NotifyWARNTitle"  :fg "#d7c489" :bold #t)
(defhighlight :group "NotifyINFOTitle"  :fg "#94bbb8")
