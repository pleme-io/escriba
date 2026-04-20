;; escriba sample rc — exercises every form `escriba-lisp` supports
;; today. Point the editor at it:
;;
;;   escriba --rc escriba/examples/sample-rc.lisp --list-rc
;;
;; The binary loads this in read-only mode and reports what it
;; would apply. Point the editor at this for real with
;;
;;   escriba --rc escriba/examples/sample-rc.lisp --render=text <file>
;;
;; to see a single-frame render using this config.

;; ── Keybindings ───────────────────────────────────────────────────
;; Re-bind normal-mode h to move right (the canonical joke) so
;; `--list-rc` proves the rc is actually being applied.
(defkeybind :mode "normal" :key "h" :action "move-right")
;; A well-known typed-variant action.
(defkeybind :mode "normal" :key "<C-q>" :action "quit")
;; Unknown action — will defer to the command registry at
;; dispatch time.
(defkeybind :mode "normal" :key "<C-p>" :action "picker.files"
            :description "open the file picker")

;; ── Theme ─────────────────────────────────────────────────────────
(deftheme :preset "nord")

;; ── Options ───────────────────────────────────────────────────────
(defoption :name "number"          :value "true")
(defoption :name "relativenumber"  :value "true")
(defoption :name "tabstop"         :value "4")

;; ── Commands ──────────────────────────────────────────────────────
(defcmd :name "write-all"
        :description "Write every modified buffer"
        :action "buffer.write-all")

;; ── Hooks ─────────────────────────────────────────────────────────
(defhook :event "BufWritePost" :command "run-formatter")
(defhook :event "ModeChanged"  :to "insert"
         :command "highlight-cursor-line")

;; ── Filetypes ─────────────────────────────────────────────────────
(defft :ext "rs"   :mode "rust")
(defft :ext "py"   :mode "python")
(defft :ext "lisp" :mode "lisp")

;; ── Abbreviations + snippets ──────────────────────────────────────
(defabbrev :trigger "teh" :expansion "the")
(defsnippet :trigger "fn"
            :body "fn ${1:name}(${2}) -> ${3} { ${0} }"
            :filetype "rust"
            :description "rust function boilerplate")
