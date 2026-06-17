;; escriba-paredit — an escriba plugin authored as a caixa.
;;
;; The caixa manifest gives the plugin its fleet identity (git-as-registry:
;; publish = `feira publish` → git tag; consume = a `:deps` entry resolved
;; into the BLAKE3 `lacre.lisp` closure). escriba itself does NOT parse this
;; file — it loads the escriba ENTRY at `escriba/plugin.lisp`. This manifest
;; is what `feira` and the rest of the caixa toolchain see.
(defcaixa :nome "escriba-paredit"
          :versao "0.1.0"
          :kind Biblioteca
          :descricao "Structural Lisp editing (paredit-style sexp motions) for escriba"
          :etiquetas ("escriba-plugin" "editing" "lisp"))
