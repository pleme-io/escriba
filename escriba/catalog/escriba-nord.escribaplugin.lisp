;; escriba-nord — the classic Nord colorscheme palette.
;; Mirrors shaunsingh/nord.nvim. The fleet default theme is vellum
;; (declared in the baseline rc); this caixa provides the Nord base16
;; palette for `(deftheme :preset "nord")` users.
(defescribaplugin
  :name          "escriba-nord"
  :version       "0.1.0"
  :category      "theming"
  :description   "Nord colorscheme — arctic, muted base16 palette"
  :blnvim-origin "shaunsingh/nord.nvim"
  :ativar-em     ("Startup")
  :priority      1000)

(defpalette :name "nord"
            :base00 "#2e3440" :base01 "#3b4252" :base02 "#434c5e"
            :base03 "#4c566a" :base04 "#d8dee9" :base05 "#e5e9f0"
            :base06 "#eceff4" :base07 "#eceff4"
            :base08 "#bf616a" :base09 "#d08770" :base0a "#ebcb8b"
            :base0b "#a3be8c" :base0c "#8fbcbb" :base0d "#88c0d0"
            :base0e "#81a1c1" :base0f "#5e81ac")
