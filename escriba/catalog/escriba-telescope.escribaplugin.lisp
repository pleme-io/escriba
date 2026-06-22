;; escriba-telescope — fuzzy finder for files / buffers / grep / symbols.
;; Mirrors nvim-telescope/telescope.nvim.
(defescribaplugin
  :name          "escriba-telescope"
  :version       "0.1.0"
  :category      "telescope"
  :description   "Fuzzy finder — files / grep / buffers / symbols / LSP refs"
  :blnvim-origin "nvim-telescope/telescope.nvim"
  :ativar-em     ("Command: Telescope"))

(defkeybind :mode "normal" :key "<leader>ff" :action "picker.files"          :description "find files")
(defkeybind :mode "normal" :key "<leader>fg" :action "picker.grep"           :description "grep workspace")
(defkeybind :mode "normal" :key "<leader>fb" :action "picker.buffers"        :description "buffer switcher")
(defkeybind :mode "normal" :key "<leader>fh" :action "picker.help"           :description "help tags")
(defkeybind :mode "normal" :key "<leader>fs" :action "picker.symbols"        :description "document symbols")
(defkeybind :mode "normal" :key "<leader>fr" :action "picker.lsp-references" :description "LSP references")
(defcmd :name "Telescope" :description "open the fuzzy picker" :action "picker.files")
