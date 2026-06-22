;; escriba-gitsigns — git gutter signs, blame, hunk staging.
;; Mirrors lewis6991/gitsigns.nvim. Owns the GitSigns + Diff highlight
;; groups (migrated out of the baseline rc).
(defescribaplugin
  :name          "escriba-gitsigns"
  :version       "0.1.0"
  :category      "git"
  :description   "Git gutter signs, blame, hunk stage / reset / preview"
  :blnvim-origin "lewis6991/gitsigns.nvim"
  :ativar-em     ("Event: BufReadPost"))

(defhook :event "BufEnter" :command "git.refresh-signs")

(defkeybind :mode "normal" :key "<leader>gb" :action "git.blame"        :description "toggle git blame")
(defkeybind :mode "normal" :key "<leader>hs" :action "git.stage-hunk"   :description "stage hunk")
(defkeybind :mode "normal" :key "<leader>hr" :action "git.reset-hunk"   :description "reset hunk")
(defkeybind :mode "normal" :key "<leader>hp" :action "git.preview-hunk" :description "preview hunk")
(defkeybind :mode "normal" :key "]c"         :action "git.next-hunk"    :description "next hunk")
(defkeybind :mode "normal" :key "[c"         :action "git.prev-hunk"    :description "previous hunk")

(defhighlight :group "GitSignsAdd"    :fg "#a9bb8c")
(defhighlight :group "GitSignsChange" :fg "#d7c489")
(defhighlight :group "GitSignsDelete" :fg "#c9837b")
(defhighlight :group "DiffAdd"        :bg "#4d543e")
(defhighlight :group "DiffChange"     :bg "#595137")
(defhighlight :group "DiffDelete"     :bg "#7b4f4a")
