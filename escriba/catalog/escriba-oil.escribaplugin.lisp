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
(defkeybind :mode "normal" :key "-"         :action "files.open-parent" :description "open parent directory")
(defcmd :name "Oil" :description "open the file explorer at the cwd" :action "files.open")
