use std::path::{Path, PathBuf};

const AREAS: &[&str] = &[
    "core", "buffers", "editing", "find", "lsp", "syntax", "git", "ui", "blue", "config",
];
const STATES: &[&str] = &["built", "partial", "absent"];
const RUNGS: &[&str] = &["-", "R1", "R2", "R3", "R4", "R5"];

fn root() -> PathBuf {
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    loop {
        if dir.join("docs/parity.tsv").is_file() {
            return dir;
        }
        assert!(dir.pop(), "docs/parity.tsv is not above {}", env!("CARGO_MANIFEST_DIR"));
    }
}

fn has_test(root: &Path, cite: &str) -> bool {
    let Some((file, name)) = cite.split_once("::") else {
        return false;
    };
    let Ok(text) = std::fs::read_to_string(root.join(file)) else {
        return false;
    };
    let lines: Vec<&str> = text.lines().collect();
    let head = format!("fn {name}(");
    lines.iter().enumerate().any(|(i, l)| {
        l.trim_start().starts_with(&head)
            && lines[i.saturating_sub(3)..i].iter().any(|a| a.trim() == "#[test]")
    })
}

fn findings(root: &Path, table: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (n, line) in table.lines().enumerate().skip(1) {
        if line.trim().is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split('\t').collect();
        let at = n + 1;
        if f.len() != 6 {
            out.push(format!("row {at}: {} fields, the table has 6", f.len()));
            continue;
        }
        let (area, feature, blnvim, state, test, rung) = (f[0], f[1], f[2], f[3], f[4], f[5]);
        if !AREAS.contains(&area) {
            out.push(format!("row {at} ({feature}): area `{area}` is not one of {AREAS:?}"));
        }
        if !STATES.contains(&state) {
            out.push(format!("row {at} ({feature}): state `{state}` is not one of {STATES:?}"));
        }
        if !RUNGS.contains(&rung) {
            out.push(format!("row {at} ({feature}): rung `{rung}` is not one of {RUNGS:?}"));
        }
        if blnvim.trim().is_empty() {
            out.push(format!("row {at} ({feature}): no blnvim or vim source named"));
        }
        match (state, test) {
            ("built", "-") => out.push(format!("row {at} ({feature}): built with no test")),
            ("absent", t) if t != "-" => {
                out.push(format!("row {at} ({feature}): absent and claims test {t}"))
            }
            (_, "-") => {}
            (_, t) if !has_test(root, t) => {
                out.push(format!("row {at} ({feature}): {t} is not a #[test] fn"))
            }
            _ => {}
        }
        if state == "built" && rung != "-" {
            out.push(format!("row {at} ({feature}): built and still on rung {rung}"));
        }
        if state != "built" && rung == "-" {
            out.push(format!("row {at} ({feature}): {state} with no rung to close it"));
        }
    }
    out
}

#[test]
fn every_parity_row_is_held_by_a_test_or_owed_by_a_rung() {
    let root = root();
    let table = std::fs::read_to_string(root.join("docs/parity.tsv")).unwrap();
    let found = findings(&root, &table);
    assert!(found.is_empty(), "{}", found.join("\n"));
}

#[test]
fn the_parity_gate_goes_red_on_each_bad_row() {
    let root = root();
    let head = "area\tfeature\tblnvim\tstate\ttest\trung\n";
    let cases = [
        "core\tx\tvim\tbuilt\t-\t-",
        "core\tx\tvim\tbuilt\tescriba/tests/parity_matrix.rs::no_such_test\t-",
        "core\tx\tvim\tabsent\tescriba/tests/parity_matrix.rs::the_parity_gate_goes_red_on_each_bad_row\tR1",
        "core\tx\tvim\tdone\t-\tR1",
        "nowhere\tx\tvim\tabsent\t-\tR1",
        "core\tx\t \tabsent\t-\tR1",
        "core\tx\tvim\tabsent\t-\t-",
        "core\tx\tvim\tbuilt\tescriba/tests/parity_matrix.rs::the_parity_gate_goes_red_on_each_bad_row\tR2",
        "core\tx\tvim\tabsent",
        "core\tx\tvim\tbuilt\tescriba/tests/parity_matrix.rs::root\t-",
    ];
    for case in cases {
        assert!(
            !findings(&root, &format!("{head}{case}\n")).is_empty(),
            "the gate passed a bad row: {case}"
        );
    }
    let good = "core\tx\tvim\tbuilt\tescriba/tests/parity_matrix.rs::the_parity_gate_goes_red_on_each_bad_row\t-";
    assert!(findings(&root, &format!("{head}{good}\n")).is_empty());
}
