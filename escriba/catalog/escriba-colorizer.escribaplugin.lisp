;; escriba-colorizer — inline color swatches for hex / rgb / hsl.
;; Mirrors NvChad/nvim-colorizer.lua.
(defescribaplugin
  :name          "escriba-colorizer"
  :version       "0.1.0"
  :category      "theming"
  :description   "Inline hex / rgb / hsl color swatches"
  :blnvim-origin "NvChad/nvim-colorizer.lua"
  :ativar-em     ("Event: BufReadPost"))

(defoption :name "colorizer.enabled" :value "true")
(defcmd :name "ColorizerToggle" :description "toggle inline color swatches" :action "colorizer.toggle")
