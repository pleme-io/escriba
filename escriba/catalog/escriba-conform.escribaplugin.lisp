;; escriba-conform — pluggable formatter runner (format-on-save).
;; Mirrors stevearc/conform.nvim — one formatter per filetype, run on
;; the BufWritePre hook and on demand via <leader>lf.
(defescribaplugin
  :name          "escriba-conform"
  :version       "0.1.0"
  :category      "formatting"
  :description   "Pluggable formatter runner — format on save + on demand"
  :blnvim-origin "stevearc/conform.nvim"
  :ativar-em     ("Startup"))

;; Format-on-save + on-demand format.
(defhook :event "BufWritePre" :command "lsp.format-if-enabled")
(defkeybind :mode "normal" :key "<leader>lf" :action "lsp.format" :description "format buffer")
(defcmd :name "Format" :description "format the active buffer" :action "lsp.format")

;; ── Formatters (conform.nvim parity) ─────────────────────────────────
(defformatter :filetype "rust"       :command "rustfmt"  :args ("--edition" "2024"))
(defformatter :filetype "python"     :command "ruff"     :args ("format" "-"))
(defformatter :filetype "typescript" :command "prettier" :args ("--stdin-filepath" "$FILE"))
(defformatter :filetype "javascript" :command "prettier" :args ("--stdin-filepath" "$FILE"))
(defformatter :filetype "lua"        :command "stylua"   :args ("-"))
(defformatter :filetype "nix"        :command "alejandra")
(defformatter :filetype "go"         :command "gofmt")
(defformatter :filetype "terraform"  :command "terraform" :args ("fmt" "-"))
(defformatter :filetype "yaml"       :command "prettier" :args ("--parser" "yaml"))
(defformatter :filetype "markdown"   :command "prettier" :args ("--parser" "markdown"))
(defformatter :filetype "sh"         :command "shfmt"    :args ("-i" "2"))

;; ── tatara-lisp ──────────────────────────────────────────────────────
;;
;; The editor is written IN this language and could not format it. That
;; gap was the whole reason a .tlisp buffer drifted.
;;
;; `feira fmt` writes in place — caixa-fmt is a library with no binary of
;; its own, and `feira` is its only CLI — so `$FILE` is passed rather
;; than piping stdin. Do NOT switch this to `--stdout`: that flag is a
;; filter and prints the document every time, but the rest of this file
;; pipes stdin, and mixing the two conventions per-entry is how a
;; formatter table starts disagreeing with itself.
(defformatter :filetype "tlisp"      :command "feira"    :args ("fmt" "$FILE"))

;; ── blue ─────────────────────────────────────────────────────────────
;;
;; Same convention as tlisp above, and for the same reason: `blue fmt`
;; takes a FILE, not stdin, so `$FILE` is passed rather than piping.
;;
;; `--write` is REQUIRED, not decoration. Bare `blue fmt FILE` is a
;; filter — it prints the formatted document to stdout and leaves the
;; file untouched — so without the flag this entry would format nothing
;; and report success, the quietest possible failure. `--write` and
;; `--check` both run `format_source_lossless`, so the rewrite preserves
;; comments; the comment-stripping path is not reachable from the CLI.
(defformatter :filetype "blue"       :command "blue"     :args ("fmt" "--write" "$FILE"))

;; ── The canonicality gate ────────────────────────────────────────────
;;
;; tatara-lisp has exactly ONE canonical rendering — the layout space is
;; a closed `FormShape` set in caixa-fmt, so every form has one answer and
;; there is nothing to configure. `caixa_fmt::parse_canonical` makes that
;; a parse-time property: it parses, re-renders, compares bytes, and
;; refuses to yield an AST when they differ.
;;
;; This gate is the EDITOR half of the same invariant, and `auto-fix` is
;; the RIGHT action here — not a concession. The two layers do different
;; jobs: `parse_canonical` is the refusal (a non-canonical file does not
;; run), and this gate is the repair (a save leaves the file canonical).
;; An editor that only complained would leave the operator hand-fixing
;; what the formatter fixes exactly, and a `reject` here would buy no
;; extra safety — the language already refuses.
;;
;; The fleet corpus is canonical as of the mass-format: 568 canonical,
;; 0 non-canonical, 3 unparseable (a pre-existing unterminated-string
;; lexer limit). Before that commit it was 72 / 496, which is why this
;; gate could not have been `reject` even if that were desirable.
;;
;; STILL OPEN: `pleme-doc-gen` emits tatara-lisp by hand-building strings
;; and depends on no caixa crate, so generated caixa.lisp files re-drift
;; whenever it runs. This gate is what makes that drift LOUD instead of
;; silent. `pending-tlisp-emitter: canonical-output`.
(defgate :name       "tlisp-canonical"
         :on-event   "BufWritePre"
         :filetype   "tlisp"
         :command    "feira fmt --check $FILE"
         :action     "auto-fix"
         :auto-fix   "feira fmt $FILE"
         :message    "tatara-lisp has one canonical form and evaluation is gated on it — reformatted.")

;; ── The same gate, for blue ──────────────────────────────────────────
;;
;; blue makes the identical promise tatara-lisp does — `blue fmt`'s own
;; help calls it "the one formatting; this produces it", and the CLI has
;; no width or style knob to disagree with. So the editor half is the
;; same shape, and `auto-fix` is right here for the same reason: a
;; `reject` would buy nothing, because the repair is exactly determined.
;;
;; It differs from the tlisp gate in one honest way. tlisp's refusal is
;; at the LANGUAGE — `caixa_fmt::parse_canonical` will not yield an AST
;; for a non-canonical file, so a drifted .tlisp does not run. blue has
;; no such parse-time refusal: `blue run` happily executes an unformatted
;; program. That makes this gate the ONLY thing holding the line for .b
;; files, so it is a convention the editor keeps, not an invariant the
;; language enforces — only-mitigated, and it should not be read as more.
(defgate :name       "blue-canonical"
         :on-event   "BufWritePre"
         :filetype   "blue"
         :command    "blue fmt --check $FILE"
         :action     "auto-fix"
         :auto-fix   "blue fmt --write $FILE"
         :message    "blue has one formatting — reformatted.")
