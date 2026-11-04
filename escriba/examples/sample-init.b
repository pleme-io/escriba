use("escriba", %i[alias bind command form highlight hook off option theme tune])

def escriba()
  [
    theme("nord"),
    option("number", true),
    option("tabstop", 4),
    option("relativenumber", true),
    tune(:defoption, "tabstop", {value: "2"}),
    off(:defoption, "relativenumber"),
    highlight("Comment", {fg: "#616E88", italic: true}),
    bind("normal", "gh", :whereami),
    alias("w2", "Write the buffer", "buffer.save"),
    command("whereami", "Say where the cursor is", :whereami),
    hook("BufWritePost", "whereami"),
    form(:defruler, "wide", {columns: [100]})
  ]
end

def whereami(args)
  message(
    "#{buffer_name} #{cursor_line + 1}:#{cursor_column + 1} #{editor_mode}"
  )
end
