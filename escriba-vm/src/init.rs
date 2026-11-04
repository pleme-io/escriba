use std::path::{Path, PathBuf};
use std::sync::Arc;

use blue_lang_host::world::{budget, loader_for};
use blue_lang_host::{
    Canonical, Capability, Embedded, EmbeddedOver, Entry, Error, MapKey, Program, Value,
    domain_form,
};
use escriba_madoguchi::Negai;
use serde::{Deserialize, Serialize};

use crate::{EditorSnapshot, EscribaHost, VmError};

pub const LOAD_FUEL: usize = 20_000_000;
pub const CALL_FUEL: usize = 5_000_000;
pub const ENTRY_FUNCTION: &str = "escriba";
const LINE_FUNCTION: &str = "__escriba_blue_line";

const BIDAMA: Embedded<'static> = Embedded {
    name: "escriba",
    files: &[("escriba.b", include_str!("../bidamas/escriba/escriba.b"))],
};

const NO_HOST_EFFECT: &[Capability] = &[];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Form {
    pub keyword: String,
    pub name: String,
    pub lisp: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlueCommand {
    pub name: String,
    pub description: String,
    pub call: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Effect {
    Message(String),
    Insert(String),
    SetOption { name: String, value: String },
    RunCommand { name: String, args: Vec<String> },
}

impl From<Effect> for Negai {
    fn from(effect: Effect) -> Self {
        match effect {
            Effect::Message(text) => Self::Message(text),
            Effect::Insert(text) => Self::InsertText(text),
            Effect::SetOption { name, value } => Self::SetOption { name, value },
            Effect::RunCommand { name, args } => Self::RunCommand { name, args },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Request {
    Load {
        path: PathBuf,
        keywords: Vec<String>,
        snapshot: EditorSnapshot,
    },
    Call {
        path: PathBuf,
        function: String,
        args: Vec<String>,
        snapshot: EditorSnapshot,
    },
    Line {
        path: Option<PathBuf>,
        line: String,
        snapshot: EditorSnapshot,
    },
    Script {
        source: String,
        snapshot: EditorSnapshot,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reply {
    pub effects: Vec<Effect>,
    pub forms: Vec<Form>,
    pub commands: Vec<BlueCommand>,
    pub off: Vec<String>,
    pub refusals: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Wire {
    Replied(Reply),
    Failed(String),
}

fn load_program(
    entry: Entry<'_>,
    host: &mut EscribaHost,
    fuel: usize,
) -> Result<Program<EscribaHost>, Error> {
    let rest = entry
        .path
        .map_or_else(|| loader_for(Path::new("init.b")), loader_for);
    let loader = EmbeddedOver {
        own: BIDAMA,
        rest: &rest,
    };
    let permits = |_: &str| true;
    Program::<EscribaHost>::builder(entry)
        .loader(&loader)
        .canonical(Canonical::AsWritten)
        .posture(NO_HOST_EFFECT)
        .frame(&permits, budget(fuel))
        .load(host)
}

fn judged(keywords: &[String], entries: Value, fuel: usize) -> Result<Value, Error> {
    let names: Vec<String> = keywords.iter().map(|k| format!(":{k}")).collect();
    let source = format!(
        "use(\"escriba\", [:declare])\n\ndef judged(entries)\n  declare([{}], entries)\nend\n",
        names.join(", ")
    );
    let mut host = EscribaHost::new();
    let mut judge = load_program(Entry::anonymous(&source), &mut host, fuel)?;
    judge.call_budgeted::<Value>("judged", (entries,), &mut host, budget(fuel))
}

fn field<'v>(record: &'v Value, name: &str) -> Option<&'v Value> {
    let Value::Map(map) = record else {
        return None;
    };
    map.iter().find_map(|(key, value)| match key {
        MapKey::Keyword(k) | MapKey::Symbol(k) if k.as_ref() == name => Some(value),
        _ => None,
    })
}

fn text(value: Option<&Value>) -> String {
    match value {
        Some(Value::Str(s) | Value::Keyword(s) | Value::Symbol(s)) => s.to_string(),
        _ => String::new(),
    }
}

fn list(value: Option<&Value>) -> Vec<Value> {
    match value {
        Some(Value::List(items)) => items.iter().cloned().collect(),
        _ => Vec::new(),
    }
}

fn read_entry(entry: &Value, program: &Program<EscribaHost>, reply: &mut Reply) {
    let kind = text(field(entry, "entry"));
    let name = text(field(entry, "name"));
    let value = field(entry, "value").cloned().unwrap_or(Value::Nil);
    if kind == "command" {
        let call = text(field(&value, "call"));
        if program.hosted().defines(&call) {
            reply.commands.push(BlueCommand {
                name,
                description: text(field(&value, "description")),
                call,
            });
        } else {
            reply
                .refusals
                .push(format!("command {name}: no function named {call}"));
        }
        return;
    }
    match domain_form(&kind, &value) {
        Ok(form) => reply.forms.push(Form {
            keyword: kind,
            name,
            lisp: form.to_string(),
        }),
        Err(e) => reply.refusals.push(format!("{kind} {name}: {e}")),
    }
}

fn effects_of(host: &mut EscribaHost) -> Vec<Effect> {
    host.take_effects()
        .into_iter()
        .filter_map(|negai| match negai {
            Negai::Message(text) => Some(Effect::Message(text)),
            Negai::InsertText(text) => Some(Effect::Insert(text)),
            Negai::SetOption { name, value } => Some(Effect::SetOption { name, value }),
            Negai::RunCommand { name, args } => Some(Effect::RunCommand { name, args }),
            _ => None,
        })
        .collect()
}

fn read_source(path: &Path) -> Result<String, VmError> {
    std::fs::read_to_string(path).map_err(|e| VmError::Worker(format!("{}: {e}", path.display())))
}

fn load(
    path: &Path,
    keywords: &[String],
    snapshot: &EditorSnapshot,
    reply: &mut Reply,
) -> Result<(), VmError> {
    let source = read_source(path)?;
    let mut host = EscribaHost::with_snapshot(snapshot.clone());
    let entry = Entry {
        path: Some(path),
        text: &source,
    };
    let mut program = load_program(entry, &mut host, LOAD_FUEL)?;
    if program.hosted().defines(ENTRY_FUNCTION) {
        let found = program
            .call_budgeted::<Value>(ENTRY_FUNCTION, (), &mut host, budget(LOAD_FUEL))
            .and_then(|entries| judged(keywords, entries, LOAD_FUEL));
        match found {
            Ok(judged) => {
                for refusal in list(field(&judged, "refusals")) {
                    reply.refusals.push(format!(
                        "{}: {}",
                        text(field(&refusal, "kind")),
                        text(field(&refusal, "why"))
                    ));
                }
                for entry in list(field(&judged, "off")) {
                    reply.off.push(text(field(&entry, "name")));
                }
                for entry in list(field(&judged, "accepted")) {
                    read_entry(&entry, &program, reply);
                }
            }
            Err(e) => reply.refusals.push(e.to_string()),
        }
    }
    reply.effects = effects_of(&mut host);
    Ok(())
}

fn call(
    path: &Path,
    function: &str,
    args: &[String],
    snapshot: &EditorSnapshot,
) -> Result<Vec<Effect>, VmError> {
    let source = read_source(path)?;
    let mut host = EscribaHost::with_snapshot(snapshot.clone());
    let entry = Entry {
        path: Some(path),
        text: &source,
    };
    let mut program = load_program(entry, &mut host, CALL_FUEL)?;
    host.effects.clear();
    let args = Value::list(args.iter().map(|a| Value::Str(Arc::from(a.as_str()))));
    program.call_budgeted::<Value>(function, (args,), &mut host, budget(CALL_FUEL))?;
    Ok(effects_of(&mut host))
}

fn line(
    path: Option<&Path>,
    source_line: &str,
    snapshot: &EditorSnapshot,
) -> Result<Vec<Effect>, VmError> {
    let prelude = path.map(read_source).transpose()?.unwrap_or_default();
    let source = format!("{prelude}\n\ndef {LINE_FUNCTION}()\n{source_line}\nend\n");
    let mut host = EscribaHost::with_snapshot(snapshot.clone());
    let entry = Entry {
        path,
        text: &source,
    };
    let mut program = load_program(entry, &mut host, CALL_FUEL)?;
    host.effects.clear();
    program.call_budgeted::<Value>(LINE_FUNCTION, (), &mut host, budget(CALL_FUEL))?;
    Ok(effects_of(&mut host))
}

pub(crate) fn eval_top(src: &str, host: &mut EscribaHost) -> Result<(), VmError> {
    load_program(Entry::anonymous(src), host, CALL_FUEL)?;
    Ok(())
}

pub fn serve(request: &Request) -> Result<Reply, VmError> {
    let mut reply = Reply::default();
    match request {
        Request::Load {
            path,
            keywords,
            snapshot,
        } => load(path, keywords, snapshot, &mut reply)?,
        Request::Call {
            path,
            function,
            args,
            snapshot,
        } => reply.effects = call(path, function, args, snapshot)?,
        Request::Line {
            path,
            line: source_line,
            snapshot,
        } => reply.effects = line(path.as_deref(), source_line, snapshot)?,
        Request::Script { source, snapshot } => {
            let mut host = EscribaHost::with_snapshot(snapshot.clone());
            eval_top(source, &mut host)?;
            reply.effects = effects_of(&mut host);
        }
    }
    Ok(reply)
}
