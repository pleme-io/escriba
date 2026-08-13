;; escriba-oil — edit the filesystem like a buffer.
;; Mirrors stevearc/oil.nvim.
(defescribaplugin
  :name          "escriba-oil"
  :version       "0.1.0"
  :category      "files"
  :description   "Edit the filesystem like a buffer"
  :blnvim-origin "stevearc/oil.nvim"
  :ativar-em     ("Command: Oil"))

(defkeybind :mode "normal" :key "<leader>e" :action "files.open"        :description "open file explorer")
;; `<leader>-`, NOT a bare `-` (2026-08-13). oil.nvim takes `-` in neovim and
;; pays for it by losing the motion; escriba's `-` is vim's
;; previous-line-first-non-blank, and a bare binding here displaced it in the
;; shipped build while every unit test stayed green. The file browser is not
;; wired yet, so this traded a working motion for an inert key.
(defkeybind :mode "normal" :key "<leader>-" :action "files.open-parent" :description "open parent directory")
(defcmd :name "Oil" :description "open the file explorer at the cwd" :action "files.open")
