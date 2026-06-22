;; escriba-which-key — pending-keybinding popup.
;; Mirrors folke/which-key.nvim: after the leader prefix, show the
;; available completions.
(defescribaplugin
  :name          "escriba-which-key"
  :version       "0.1.0"
  :category      "common"
  :description   "Popup showing pending keybinding completions"
  :blnvim-origin "folke/which-key.nvim"
  :ativar-em     ("Startup")
  :priority      900)

(defoption :name "which-key.enabled"  :value "true")
(defoption :name "which-key.delay-ms" :value "300")
(defkeybind :mode "normal" :key "<leader>?" :action "whichkey.show" :description "show keybinding help")
(defcmd :name "WhichKey" :description "show the which-key popup" :action "whichkey.show")
