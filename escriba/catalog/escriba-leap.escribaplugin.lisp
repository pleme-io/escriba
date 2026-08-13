;; escriba-leap — label-based motion (leap to anywhere on screen).
;; Mirrors ggandor/leap.nvim.
;;
;; MOVED off `s`/`S` on 2026-08-13, and it is the same call as `<S-h>`/`<S-l>`
;; (bufferline), `-` (oil) and `<C-h>` (luasnip) before it: a bundled caixa is
;; applied ON TOP of the default keymap, so upstream's own choice of key
;; silently displaced a CORE vim verb — `s` (substitute char) and `S`
;; (substitute line). leap.nvim taking `s` upstream is a known-controversial
;; choice there and simply not one escriba can inherit, because leap is not
;; wired: it traded two working edit verbs for two dead keys.
;;
;; `escriba/tests/movement_survives_defaults.rs` is the gate that caught it and
;; will catch the next one.
(defescribaplugin
  :name          "escriba-leap"
  :version       "0.1.0"
  :category      "common"
  :description   "Label-based motion — leap forward / backward to any target"
  :blnvim-origin "ggandor/leap.nvim"
  :ativar-em     ("Startup"))

(defkeybind :mode "normal" :key "<leader>s" :action "leap.forward"  :description "leap forward")
(defkeybind :mode "normal" :key "<leader>S" :action "leap.backward" :description "leap backward")
(defcmd :name "Leap" :description "start a leap motion" :action "leap.forward")
