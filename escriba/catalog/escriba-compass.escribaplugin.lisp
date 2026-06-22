;; escriba-compass — workspace-aware project navigation + pane focus.
;; Mirrors pleme-io/compass.nvim, and ABSORBS christoomey/vim-tmux-navigator
;; — the <C-hjkl> pane bindings below are the seamless vim↔tmux navigation
;; vim-tmux-navigator provides, so escriba needs no separate caixa for it.
(defescribaplugin
  :name          "escriba-compass"
  :version       "0.1.0"
  :category      "telescope"
  :description   "Workspace-aware project picker + cross-pane (tmux) navigation"
  :blnvim-origin "pleme-io/compass.nvim"
  :ativar-em     ("Startup")
  :tags          ("christoomey/vim-tmux-navigator"))

(defkeybind :mode "normal" :key "<leader>fp" :action "picker.project" :description "find project")
(defkeybind :mode "normal" :key "<C-h>"      :action "pane.left"      :description "focus left pane / tmux")
(defkeybind :mode "normal" :key "<C-j>"      :action "pane.down"      :description "focus below pane / tmux")
(defkeybind :mode "normal" :key "<C-k>"      :action "pane.up"        :description "focus above pane / tmux")
(defkeybind :mode "normal" :key "<C-l>"      :action "pane.right"     :description "focus right pane / tmux")
(defcmd :name "Compass" :description "open the project picker" :action "picker.project")
