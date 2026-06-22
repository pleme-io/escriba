;; escriba-dap — Debug Adapter Protocol client + common adapters.
;; Mirrors mfussenegger/nvim-dap + nvim-dap-ui.
(defescribaplugin
  :name          "escriba-dap"
  :version       "0.1.0"
  :category      "lsp"
  :description   "Debug Adapter Protocol — breakpoints, stepping, watches"
  :blnvim-origin "mfussenegger/nvim-dap"
  :ativar-em     ("Command: DapContinue"))

;; ── Debug keybinds (<leader>d…) ──────────────────────────────────────
(defkeybind :mode "normal" :key "<leader>db" :action "dap.toggle-breakpoint" :description "toggle breakpoint")
(defkeybind :mode "normal" :key "<leader>dc" :action "dap.continue"          :description "continue / start")
(defkeybind :mode "normal" :key "<leader>do" :action "dap.step-over"         :description "step over")
(defkeybind :mode "normal" :key "<leader>di" :action "dap.step-into"         :description "step into")
(defkeybind :mode "normal" :key "<leader>dO" :action "dap.step-out"          :description "step out")
(defkeybind :mode "normal" :key "<leader>du" :action "dap.toggle-ui"         :description "toggle dap-ui")
(defkeybind :mode "normal" :key "<leader>dr" :action "dap.repl"              :description "open dap repl")

(defcmd :name "DapContinue"        :description "continue / start debugging" :action "dap.continue")
(defcmd :name "DapToggleBreakpoint" :description "toggle a breakpoint"       :action "dap.toggle-breakpoint")

;; ── Adapters (nvim-dap parity, common languages) ─────────────────────
(defdap :name "lldb"
        :command "lldb-dap"
        :filetypes ("rust" "c" "cpp"))
(defdap :name "codelldb"
        :command "codelldb"
        :filetypes ("rust" "c" "cpp"))
(defdap :name "debugpy"
        :command "python"
        :args ("-m" "debugpy.adapter")
        :filetypes ("python"))
(defdap :name "delve"
        :command "dlv"
        :args ("dap" "-l" "127.0.0.1:38697")
        :filetypes ("go")
        :port 38697)
(defdap :name "js-debug"
        :command "node"
        :args ("-e" "require('@vscode/js-debug')")
        :filetypes ("typescript" "javascript"))
