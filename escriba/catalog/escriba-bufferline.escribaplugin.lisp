;; escriba-bufferline — buffer / tab line.
;; Mirrors akinsho/bufferline.nvim.
(defescribaplugin
  :name          "escriba-bufferline"
  :version       "0.1.0"
  :category      "theming"
  :description   "Buffer / tab line with diagnostic + modified indicators"
  :blnvim-origin "akinsho/bufferline.nvim"
  :ativar-em     ("Startup"))

(defkeybind :mode "normal" :key "<S-h>" :action "buffer.prev" :description "previous buffer")
(defkeybind :mode "normal" :key "<S-l>" :action "buffer.next" :description "next buffer")

(defbufferline
  :separator "│"
  :modified-indicator "●"
  :show-close-icons #t
  :show-diagnostics #t
  :max-name-length 20)
