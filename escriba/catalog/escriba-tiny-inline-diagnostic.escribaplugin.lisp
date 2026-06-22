;; escriba-tiny-inline-diagnostic — compact inline virtual-text diagnostics.
;; Mirrors rachartier/tiny-inline-diagnostic.nvim. Owns the Diagnostic
;; highlight groups (migrated out of the baseline rc).
(defescribaplugin
  :name          "escriba-tiny-inline-diagnostic"
  :version       "0.1.0"
  :category      "lsp"
  :description   "Compact inline virtual-text diagnostics"
  :blnvim-origin "rachartier/tiny-inline-diagnostic.nvim"
  :ativar-em     ("Event: LspAttach"))

(defoption :name "diagnostics.inline" :value "true")

(defhighlight :group "DiagnosticError" :fg "#c9837b" :bold #t)
(defhighlight :group "DiagnosticWarn"  :fg "#d7c489")
(defhighlight :group "DiagnosticInfo"  :fg "#94bbb8")
(defhighlight :group "DiagnosticHint"  :fg "#a9bb8c")
