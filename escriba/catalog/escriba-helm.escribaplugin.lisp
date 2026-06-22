;; escriba-helm — Helm chart / Kubernetes manifest support.
;; Mirrors towolf/vim-helm: a helm major-mode + the helm language server.
(defescribaplugin
  :name          "escriba-helm"
  :version       "0.1.0"
  :category      "lsp"
  :description   "Helm template + Kubernetes manifest syntax + LSP (helm-ls)"
  :blnvim-origin "towolf/vim-helm"
  :ativar-em     ("FileType: helm"))

(defmode :name "helm"
         :extensions ("tpl")
         :tree-sitter "gotmpl"
         :commentstring "{{/* %s */}}"
         :indent 2)

(deflsp :name "helm-ls"
        :command "helm_ls"
        :args ("serve")
        :filetypes ("helm")
        :root-markers ("Chart.yaml"))
