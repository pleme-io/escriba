;; escriba-lualine — status line.
;; Mirrors nvim-lualine/lualine.nvim — a three-slot composition whose
;; segment providers resolve against the runtime's segment table.
(defescribaplugin
  :name          "escriba-lualine"
  :version       "0.1.0"
  :category      "theming"
  :description   "Fast, configurable status line (mode / branch / diagnostics / position)"
  :blnvim-origin "nvim-lualine/lualine.nvim"
  :ativar-em     ("Startup"))

(defstatusline
  :left ((:segment "mode"   :highlight "StatusLineMode")
         (:segment "branch" :highlight "GitSignsAdd" :prefix " ")
         (:segment "file"   :highlight "StatusLine"  :prefix " "))
  :center ()
  :right ((:segment "diagnostics")
          (:segment "lsp")
          (:segment "filetype" :prefix " ")
          (:segment "position" :prefix " :")
          (:segment "time"     :format "%H:%M" :prefix " ")))
