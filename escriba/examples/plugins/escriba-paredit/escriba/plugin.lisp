;; escriba-paredit — escriba ENTRY.
;;
;; This is the tatara-lisp escriba LOADS and APPLIES when the plugin
;; activates (eagerly, or on its `:ativar-em` trigger — e.g. FileType: lisp).
;; It is plain escriba-lisp: the same def-forms a user writes in their rc,
;; so a plugin is "just authored editor config, shipped as a caixa".

;; Emacs-paredit-style structural motions, bound under Alt like paredit.
(defkeybind :mode "normal" :key "<A-f>" :action "forward-sexp"        :description "paredit: forward sexp")
(defkeybind :mode "normal" :key "<A-b>" :action "backward-sexp"       :description "paredit: backward sexp")
(defkeybind :mode "normal" :key "<A-u>" :action "up-list"             :description "paredit: up list")
(defkeybind :mode "normal" :key "<A-d>" :action "down-list"           :description "paredit: down list")

;; A leader sequence for jumping to the enclosing defun's head.
(defkeybind :mode "normal" :key "<leader>cb" :action "beginning-of-defun" :description "paredit: beginning of defun")

;; A palette command the plugin contributes.
(defcmd :name "paredit-wrap-round"
        :description "Wrap the sexp at point in parentheses"
        :action "paredit.wrap-round")

;; A plugin-set option (lands in the shared option store).
(defoption :name "paredit" :value "on")
