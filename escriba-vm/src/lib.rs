//! `escriba-vm` — tatara-lisp plugin evaluator host. Phase-1 skeleton;
//! phase 2 wires a terreiro-sandboxed VM.

extern crate self as escriba_vm;

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum VmError {
    #[error("lisp: {0}")]
    Lisp(#[from] tatara_lisp::LispError),
    #[error("not implemented (phase 2): {0}")]
    NotImplemented(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginContext {
    pub caixa: String,
    pub versao: String,
}

pub trait PluginVm: Send + Sync {
    fn evaluate_plugin(&mut self, ctx: &PluginContext, source: &str) -> Result<(), VmError>;
}

#[derive(Debug, Default)]
pub struct SkeletonVm;

impl PluginVm for SkeletonVm {
    fn evaluate_plugin(&mut self, _ctx: &PluginContext, source: &str) -> Result<(), VmError> {
        let _ = tatara_lisp::read(source)?;
        Err(VmError::NotImplemented(
            "plugin evaluation wired in phase 2".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skeleton_vm_parses_and_rejects() {
        let mut vm = SkeletonVm;
        let ctx = PluginContext {
            caixa: "demo".into(),
            versao: "0.1.0".into(),
        };
        let err = vm.evaluate_plugin(&ctx, "(hello)").unwrap_err();
        assert!(matches!(err, VmError::NotImplemented(_)));
    }

    #[test]
    fn skeleton_vm_fails_on_bad_lisp() {
        let mut vm = SkeletonVm;
        let ctx = PluginContext {
            caixa: "demo".into(),
            versao: "0.1.0".into(),
        };
        let err = vm.evaluate_plugin(&ctx, "(((((").unwrap_err();
        assert!(matches!(err, VmError::Lisp(_)));
    }
}
