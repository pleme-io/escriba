use std::io::Read;
use std::process::Command;
use std::time::Duration;

use blue_lang_host::world::{self, Bounded};
use escriba_vm::VmError;
use escriba_vm::init::{Reply, Request, Wire, serve};

pub const LOAD_DEADLINE: Duration = Duration::from_secs(3);
pub const CALL_DEADLINE: Duration = Duration::from_secs(2);
const MEMORY_MB: u64 = 1024;

pub fn run_worker() -> i32 {
    world::bound_memory(MEMORY_MB);
    let mut input = String::new();
    if std::io::stdin().read_to_string(&mut input).is_err() {
        return 1;
    }
    let wire = match serde_json::from_str::<Request>(&input) {
        Ok(request) => match serve(&request) {
            Ok(reply) => Wire::Replied(reply),
            Err(e) => Wire::Failed(e.to_string()),
        },
        Err(e) => Wire::Failed(e.to_string()),
    };
    match serde_json::to_string(&wire) {
        Ok(line) => {
            println!("{line}");
            0
        }
        Err(_) => 1,
    }
}

fn deadline(request: &Request) -> Duration {
    match request {
        Request::Load { .. } => LOAD_DEADLINE,
        Request::Call { .. } | Request::Line { .. } | Request::Script { .. } => CALL_DEADLINE,
    }
}

pub fn supervised(request: &Request) -> Result<Reply, VmError> {
    let exe = std::env::current_exe().map_err(|e| VmError::Worker(e.to_string()))?;
    let input = serde_json::to_vec(request).map_err(|e| VmError::Worker(e.to_string()))?;
    let mut command = Command::new(exe);
    command.arg("blue-worker").env("RUST_LOG", "off");
    let limit = deadline(request);
    match world::run_bounded_with(command, &input, limit) {
        Bounded::Finished { stdout, status } => {
            let text = String::from_utf8_lossy(&stdout);
            match serde_json::from_str::<Wire>(text.trim()) {
                Ok(Wire::Replied(reply)) => Ok(reply),
                Ok(Wire::Failed(why)) => Err(VmError::Worker(why)),
                Err(_) => Err(VmError::Worker(format!("the blue worker died: {status}"))),
            }
        }
        Bounded::TimedOut => Err(VmError::Worker(format!(
            "blue: stopped after {} ms",
            limit.as_millis()
        ))),
        Bounded::Failed(e) => Err(VmError::Worker(e.to_string())),
    }
}
