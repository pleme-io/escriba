;; escriba-render-markdown — in-place markdown structure rendering.
;; Mirrors MeanderingProgrammer/render-markdown.nvim.
(defescribaplugin
  :name          "escriba-render-markdown"
  :version       "0.1.0"
  :category      "treesitter"
  :description   "Render markdown headings / tables / lists in place"
  :blnvim-origin "MeanderingProgrammer/render-markdown.nvim"
  :ativar-em     ("FileType: markdown"))

(defoption :name "render-markdown.enabled" :value "true")
(defcmd :name "RenderMarkdownToggle" :description "toggle markdown rendering" :action "markdown.toggle-render")

(defhighlight :group "@markup.heading" :fg "#99aabe" :bold #t)
(defhighlight :group "@markup.list"    :fg "#94bbb8")
(defhighlight :group "@markup.raw"     :fg "#a9bb8c")
