//! `escriba-spec-dump` — emit the full OpenAPI 3.1 spec.
fn main() {
    let spec = escriba_spec::build_spec();
    let yaml = std::env::args().any(|a| a == "--yaml");
    if yaml {
        print!("{}", spec.to_yaml());
    } else {
        println!("{}", spec.to_json_pretty());
    }
}
