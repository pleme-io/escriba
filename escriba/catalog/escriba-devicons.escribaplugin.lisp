;; escriba-devicons — filetype + filename glyphs.
;; Mirrors nvim-tree/nvim-web-devicons (canonical subset).
(defescribaplugin
  :name          "escriba-devicons"
  :version       "0.1.0"
  :category      "theming"
  :description   "Filetype + filename icons (nerd-font glyphs)"
  :blnvim-origin "nvim-tree/nvim-web-devicons"
  :ativar-em     ("Startup"))

(deficon :filetype "rust"       :glyph "" :fg "#dea584")
(deficon :filetype "python"     :glyph "" :fg "#ffbc03")
(deficon :filetype "javascript" :glyph "" :fg "#cbcb41")
(deficon :filetype "typescript" :glyph "" :fg "#519aba")
(deficon :filetype "go"         :glyph "" :fg "#519aba")
(deficon :filetype "lua"        :glyph "" :fg "#51a0cf")
(deficon :filetype "nix"        :glyph "" :fg "#7ebae4")
(deficon :filetype "lisp"       :glyph "" :fg "#87af5f")
(deficon :filetype "markdown"   :glyph "" :fg "#519aba")
(deficon :filetype "yaml"       :glyph "" :fg "#6d8086")
(deficon :filetype "toml"       :glyph "" :fg "#9c4221")
(deficon :filetype "json"       :glyph "" :fg "#cbcb41")
(deficon :filetype "sh"         :glyph "" :fg "#89e051")
(deficon :filetype "terraform"  :glyph "" :fg "#5f43e9")
(deficon :pattern "Cargo.toml"   :glyph "" :fg "#dea584")
(deficon :pattern "Cargo.lock"   :glyph "" :fg "#dea584")
(deficon :pattern "flake.nix"    :glyph "" :fg "#7ebae4")
(deficon :pattern "flake.lock"   :glyph "" :fg "#7ebae4")
(deficon :pattern "package.json" :glyph "" :fg "#e8274b")
(deficon :pattern "Makefile"     :glyph "" :fg "#6d8086")
(deficon :pattern "Dockerfile"   :glyph "" :fg "#458ee6")
(deficon :pattern ".envrc"       :glyph "" :fg "#89e051")
(deficon :pattern ".gitignore"   :glyph "" :fg "#e24329")
