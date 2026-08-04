;; escriba-devicons — filetype + filename glyphs.
;;
;; Glyphs are nerd-font private-use codepoints, recovered 2026-08-04 from the
;; vendored nvim-tree/nvim-web-devicons this file mirrors. Every entry here
;; had shipped with an EMPTY glyph since the catalog landed — 23 of 23 — so the
;; icon leg was inert while `plan.icons.len() >= 20` stayed green. The `:fg`
;; colours were never lost, which is why the file read as intact; they are
;; preserved byte-for-byte below and were the evidence used to identify which
;; upstream entry each row came from. `every_bundled_icon_has_a_real_nerd_font_glyph`
;; in tests/plugin_matrix.rs now fails the build on an empty or non-PUA glyph.
;;
;; MIRRORED EXACTLY from upstream (key -> codepoint):
;;   rust rs, python py, javascript js, typescript ts, go go, lua lua,
;;   nix nix, markdown markdown, yaml yaml, toml toml, json json,
;;   terraform tf, package.json, Makefile, Dockerfile, .gitignore
;;
;; ESCRIBA'S OWN CHOICES — upstream has no entry for these keys, so the glyph
;; is a deliberate substitution rather than a mirror. Stated so nobody reads
;; them as upstream fact:
;;   sh / .envrc -> `bash` (U+E760). Upstream `sh` is U+E795 with colour
;;       #4d5a5e; this catalog's #89e051 is `bash`'s colour exactly, so these
;;       rows were always bash-flavoured and the glyph now agrees with the
;;       colour. `.envrc` has NO upstream icon at all (it falls through to the
;;       generic default in nvim) — the shell glyph is our call.
;;   lisp -> `el` (U+E632, Emacs-Lisp). Upstream has NO `lisp` key in any
;;       table; this is the nearest lisp-family mark. Siblings if this is ever
;;       revisited: scm U+F0627, clj U+E768.
;;   Cargo.* / flake.* -> the LANGUAGE glyph (rust U+E68B / nix U+F313).
;;       Upstream has no filename entry for these; nvim renders them via its
;;       multi-dot extension fallback (toml/lock/nix/lock). We diverge on
;;       purpose: this catalog's colours already say "a Cargo file is Rust"
;;       and "a flake file is Nix", and a lock glyph under a Rust colour would
;;       contradict that.
;;   blue / Bluefile -> the generic file glyph (U+F0F6, upstream's own
;;       `default_icon`) in blue's Nord-frost nord8. blue has no nerd-font
;;       brand mark, and inventing one by borrowing another language's would
;;       be a false claim; a real file glyph in blue's own colour is not.
;;       A Bluefile IS a blue program, so it gets the same pair.
(defescribaplugin
  :name          "escriba-devicons"
  :version       "0.2.0"
  :category      "theming"
  :description   "Filetype + filename icons (nerd-font glyphs)"
  :blnvim-origin "nvim-tree/nvim-web-devicons"
  :ativar-em     ("Startup"))

(deficon :filetype "rust"       :glyph "" :fg "#dea584")
(deficon :filetype "python"     :glyph "" :fg "#ffbc03")
(deficon :filetype "javascript" :glyph "" :fg "#cbcb41")
(deficon :filetype "typescript" :glyph "" :fg "#519aba")
(deficon :filetype "go"         :glyph "" :fg "#519aba")
(deficon :filetype "lua"        :glyph "" :fg "#51a0cf")
(deficon :filetype "nix"        :glyph "" :fg "#7ebae4")
(deficon :filetype "lisp"       :glyph "" :fg "#87af5f")
(deficon :filetype "markdown"   :glyph "" :fg "#519aba")
(deficon :filetype "yaml"       :glyph "" :fg "#6d8086")
(deficon :filetype "toml"       :glyph "" :fg "#9c4221")
(deficon :filetype "json"       :glyph "" :fg "#cbcb41")
(deficon :filetype "sh"         :glyph "" :fg "#89e051")
(deficon :filetype "terraform"  :glyph "" :fg "#5f43e9")
(deficon :filetype "blue"       :glyph "" :fg "#88c0d0")

(deficon :pattern "Cargo.toml"   :glyph "" :fg "#dea584")
(deficon :pattern "Cargo.lock"   :glyph "" :fg "#dea584")
(deficon :pattern "flake.nix"    :glyph "" :fg "#7ebae4")
(deficon :pattern "flake.lock"   :glyph "" :fg "#7ebae4")
(deficon :pattern "package.json" :glyph "" :fg "#e8274b")
(deficon :pattern "Makefile"     :glyph "" :fg "#6d8086")
(deficon :pattern "Dockerfile"   :glyph "󰡨" :fg "#458ee6")
(deficon :pattern ".envrc"       :glyph "" :fg "#89e051")
(deficon :pattern ".gitignore"   :glyph "" :fg "#e24329")
(deficon :pattern "Bluefile"     :glyph "" :fg "#88c0d0")
