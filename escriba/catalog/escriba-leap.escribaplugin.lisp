;; escriba-leap — label-based motion (s / S to jump anywhere).
;; Mirrors ggandor/leap.nvim.
(defescribaplugin
  :name          "escriba-leap"
  :version       "0.1.0"
  :category      "common"
  :description   "Label-based motion — leap forward / backward to any target"
  :blnvim-origin "ggandor/leap.nvim"
  :ativar-em     ("Startup"))

(defkeybind :mode "normal" :key "s" :action "leap.forward"  :description "leap forward")
(defkeybind :mode "normal" :key "S" :action "leap.backward" :description "leap backward")
(defcmd :name "Leap" :description "start a leap motion" :action "leap.forward")
