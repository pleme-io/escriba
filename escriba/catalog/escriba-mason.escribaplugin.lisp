;; escriba-mason — install + manage LSP servers, linters, formatters.
;; Mirrors williamboman/mason.nvim.
(defescribaplugin
  :name          "escriba-mason"
  :version       "0.1.0"
  :category      "lsp"
  :description   "Install + manage LSP servers, linters, formatters"
  :blnvim-origin "williamboman/mason.nvim"
  :ativar-em     ("Command: Mason"))

(defoption :name "mason.auto-install" :value "true")
(defcmd :name "Mason"        :description "open the mason package manager" :action "mason.open")
(defcmd :name "MasonInstall" :description "install a mason package"        :action "mason.install")
