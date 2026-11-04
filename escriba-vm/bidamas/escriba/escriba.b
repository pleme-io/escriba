@moduledoc """
escriba (書く者) — an editor's world as entries: bindings, commands, options and every def-form, each off, tuned or refused alone.

`~/.config/escriba/init.b` defines `escriba()`, which returns a list of these
entries. A def-form's own fields are checked by the editor's Rust spec, the same
one the lisp rc reads, so blue holds no second field table.

    declare(%i[defoption], [option("number", true)]).accepted[0].value #=> {name: "number", value: "true"}
"""

use("kousei", %i[entry field judge required text])

@doc """
form: any def-form of the editor, by its lisp keyword, a name to refer to it by, and its fields under the lisp names with `_` for `-`.

    form(:defruler, "wide", {columns: [100]}).entry #=> :defruler
"""
def form(keyword, name, fields)
  entry(keyword, name, fields)
end

@doc """
bind: a key in a mode and the action or command it runs.

    bind("normal", "gh", :hello).name #=> "normal gh"
    bind("normal", "gh", :hello).value.action #=> "hello"
"""
def bind(mode, key, action)
  form(:defkeybind, "#{mode} #{key}", {mode:, key:, action: to_s(action)})
end

@doc """
command: a command whose body is the blue function `call`, run with the command's arguments as one list.

    command("hello", "Say hello", :greet).value.call #=> "greet"
"""
def command(name, description, call)
  entry(:command, name, {description:, call: to_s(call)})
end

@doc """
alias: a command that runs an action or another command, the lisp `defcmd`.

    alias("w2", "Write", "buffer.save").entry #=> :defcmd
"""
def alias(name, description, action)
  form(:defcmd, name, {name:, description:, action: to_s(action)})
end

@doc """
option: an editor option; the value is read as text.

    option("tabstop", 4).value.value #=> "4"
"""
def option(name, value)
  form(:defoption, name, {name:, value: to_s(value)})
end

@doc """
theme: the editor's theme by its preset name.

    theme("nord").value #=> {preset: "nord"}
"""
def theme(preset)
  form(:deftheme, "theme", {preset:})
end

@doc """
highlight: a highlight group's colours and styles.

    highlight("Comment", {fg: "#616E88"}).value.group #=> "Comment"
"""
def highlight(group, fields)
  form(:defhighlight, group, {**fields, group:})
end

@doc """
hook: a command that an editor event runs.

    hook("BufWritePost", "fmt").name #=> "BufWritePost fmt"
"""
def hook(event, command_name)
  form(
    :defhook,
    "#{event} #{command_name}",
    {event:, command: to_s(command_name)}
  )
end

@doc """
off: a declared entry configured off; it stays in the file and does nothing.

    off(:defoption, "number").entry #=> :off
"""
def off(kind, name)
  kousei::off(kind, name)
end

@doc """
tune: fields laid over an entry declared earlier.

    tune(:defoption, "number", {value: "false"}).entry #=> :tune
"""
def tune(kind, name, fields)
  kousei::tune(kind, name, fields)
end

@doc """
kinds: what the editor takes: every def-form it names, open because its Rust spec checks the fields, and a blue command.

    kinds(%i[defoption])[:defoption] #=> :open
"""
def kinds(keywords)
  reduce(
    (acc, k) -> assoc(acc, k, :open),
    {
      command: [
        field(
          :description,
          (v) -> kousei::verdict(string?(v), "#{v} is not text")
        ),
        required(field(:call, text))
      ]
    },
    keywords
  )
end

@doc """
declare: a world's entries judged against the def-forms the editor names, each refusal a `kind` and a `why`.

    declare([], []).accepted #=> []
    declare([], [bind("normal", "x", :y)]).refusals[0].kind #=> :unknown_kind
"""
def declare(keywords, entries)
  judged = judge(kinds(keywords), entries)
  {
    accepted: judged[:accepted],
    off: judged[:off],
    refusals: map((r) -> {kind: r[0], why: r[1]}, judged[:refusals])
  }
end

test "a bad entry is refused alone and its siblings stand"
  d = declare(
    %i[defkeybind defoption],
    [
      option("number", true),
      bind("normal", "gh", :hello),
      bind("normal", "gh", :other),
      form(:defnothing, "x", {}),
      command("hello", "Say hello", :hello),
      entry(:command, "broken", {description: 3}),
      42
    ]
  )
  assert map((e) -> e[:name], d[:accepted]) == ["number", "normal gh", "hello"]
  assert map((r) -> r[:kind], d[:refusals]) ==
    %i[invalid duplicate unknown_kind invalid invalid]
end

test "off keeps an entry out and tune lays fields over it"
  d = declare(
    [:defoption],
    [
      option("number", "true"),
      option("wrap", "true"),
      off(:defoption, "number"),
      tune(:defoption, "wrap", {value: "false"})
    ]
  )
  assert map((e) -> e[:name], d[:off]) == ["number"]
  assert d[:accepted][0][:value] == {name: "wrap", value: "false"}
  assert d[:refusals] == []
end

test "a def-form's fields reach the host unchecked"
  d = declare([:defruler], [form(:defruler, "r", {anything: 1})])
  assert d[:refusals] == []
  assert d[:accepted][0][:value] == {anything: 1}
end
