;; escriba-overseer — task / build runner.
;; Mirrors stevearc/overseer.nvim + nvim asynctasks. Each task is one
;; shell invocation with filetype / cwd / env scope.
(defescribaplugin
  :name          "escriba-overseer"
  :version       "0.1.0"
  :category      "common"
  :description   "Task / build runner — cargo, nix, rg, project tasks"
  :blnvim-origin "stevearc/overseer.nvim"
  :ativar-em     ("Command: OverseerRun"))

(defcmd :name "OverseerRun"    :description "run a project task"   :action "task.run")
(defcmd :name "OverseerToggle" :description "toggle the task list" :action "task.toggle")

(deftask :name "cargo-test"
         :description "cargo test --workspace for the current project"
         :command "cargo"
         :args ("test" "--workspace")
         :filetype "rust"
         :env ("CARGO_TERM_COLOR=always" "RUST_LOG=warn")
         :background #t
         :keybind "<leader>rt"
         :timeout-ms 600000)

(deftask :name "cargo-check"
         :description "cargo check for fast type pass"
         :command "cargo"
         :args ("check" "--workspace" "--all-targets")
         :filetype "rust"
         :background #t
         :keybind "<leader>rc"
         :timeout-ms 180000)

(deftask :name "cargo-run"
         :description "cargo run (primary binary)"
         :command "cargo"
         :args ("run")
         :filetype "rust"
         :keybind "<leader>rr")

(deftask :name "rg-todos"
         :description "rg TODO/FIXME across the active cwd"
         :command "rg"
         :args ("-n" "--pretty" "TODO|FIXME|XXX|HACK" ".")
         :keybind "<leader>ft")
