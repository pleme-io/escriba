;; escriba-neotest — test runner framework + adapters.
;; Mirrors nvim-neotest/neotest + the rust / go / python / jest / plenary
;; adapters blnvim enables by default.
(defescribaplugin
  :name          "escriba-neotest"
  :version       "0.1.0"
  :category      "lsp"
  :description   "Test runner — run nearest / file / suite, summary + output panels"
  :blnvim-origin "nvim-neotest/neotest"
  :ativar-em     ("Command: Neotest"))

(defkeybind :mode "normal" :key "<leader>nn" :action "test.run-nearest" :description "run nearest test")
(defkeybind :mode "normal" :key "<leader>nf" :action "test.run-file"    :description "run file tests")
(defkeybind :mode "normal" :key "<leader>ns" :action "test.run-suite"   :description "run the test suite")
(defkeybind :mode "normal" :key "<leader>np" :action "test.toggle-summary" :description "toggle summary panel")
(defkeybind :mode "normal" :key "<leader>no" :action "test.toggle-output"  :description "toggle output panel")

(defcmd :name "Neotest"        :description "run the nearest test"      :action "test.run-nearest")
(defcmd :name "NeotestSummary" :description "toggle the test summary"   :action "test.toggle-summary")
