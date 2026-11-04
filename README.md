# escriba

> Rust editor, configured in blue, built on the pleme-io GPU stack

## Configuring

Write `~/.config/escriba/init.b`:

```blue
use("escriba", %i[bind command option theme])

def escriba()
  [
    theme("nord"),
    option("number", true),
    bind("normal", "gh", :hello),
    command("hello", "Say hello", :hello)
  ]
end

def hello(args)
  message("hello from #{buffer_name}")
end
```

A bad entry is refused alone, and a failing file is a message in the editor. `:blue
<source>` runs one line. The tatara-lisp rc still works, and `init.b` loads after it.
The budget, the fault model and every word: [`docs/blue.md`](docs/blue.md).

## Building

```bash
nix run .#escriba -- --help
```

## License

MIT.

