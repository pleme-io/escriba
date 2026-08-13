;; escriba-bufferline — buffer / tab line.
;; Mirrors akinsho/bufferline.nvim.
(defescribaplugin
  :name          "escriba-bufferline"
  :version       "0.1.0"
  :category      "theming"
  :description   "Buffer / tab line with diagnostic + modified indicators"
  :blnvim-origin "akinsho/bufferline.nvim"
  :ativar-em     ("Startup"))

;; `[b` / `]b`, NOT `<S-h>` / `<S-l>` (2026-08-13).
;;
;; `<S-h>` and `<S-l>` ARE `H` and `L` — vim's screen-top and screen-bottom
;; motions — and this caixa is applied on top of the default keymap, so it
;; displaced both. Nothing said so: the motions still resolved in every unit
;; test, because those build `Keymap::default_vim()` and only the composite
;; plan was wrong. Same shape as the `<C-h>` snippet shadowing.
;;
;; `[b` / `]b` is vim-unimpaired's spelling for the same pair, so the muscle
;; memory has somewhere to go. Pinned by
;; `escriba/tests/movement_survives_defaults.rs`.
(defkeybind :mode "normal" :key "[b" :action "buffer.prev" :description "previous buffer")
(defkeybind :mode "normal" :key "]b" :action "buffer.next" :description "next buffer")

(defbufferline
  :separator "│"
  :modified-indicator "●"
  :show-close-icons #t
  :show-diagnostics #t
  :max-name-length 20)
