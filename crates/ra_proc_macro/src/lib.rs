//! Client-side Proc-Macro crate
//!
//! We separate proc-macro expanding logic to an extern program to allow
//! different implementations (e.g. wasm or dylib loading). And this crate
//! is used to provide basic infrastructure  for communication between two
//! processes: Client (RA itself), Server (the external program)

mod rpc;
mod process;
pub mod msg;

use process::{ProcMacroProcessSrv, ProcMacroProcessThread};
use ra_tt::{SmolStr, Subtree};
use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
    sync::Arc,
};

pub use rpc::{ExpansionResult, ExpansionTask, ListMacrosResult, ListMacrosTask, ProcMacroKind};

#[derive(Debug, Clone)]
pub struct ProcMacroProcessExpander {
    process: Arc<ProcMacroProcessSrv>,
    dylib_path: PathBuf,
    name: SmolStr,
}

impl Eq for ProcMacroProcessExpander {}
impl PartialEq for ProcMacroProcessExpander {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.dylib_path == other.dylib_path
            && Arc::ptr_eq(&self.process, &other.process)
    }
}

impl ra_tt::TokenExpander for ProcMacroProcessExpander {
    fn expand(
        &self,
        subtree: &Subtree,
        _attr: Option<&Subtree>,
    ) -> Result<Subtree, ra_tt::ExpansionError> {
        self.process.custom_derive(&self.dylib_path, subtree, &self.name)
    }
}

#[derive(Debug)]
enum ProcMacroClientKind {
    Process { process: Arc<ProcMacroProcessSrv>, thread: ProcMacroProcessThread },
    Dummy,
}

#[derive(Debug)]
pub struct ProcMacroClient {
    kind: ProcMacroClientKind,
}

impl ProcMacroClient {
    pub fn extern_process<I, S>(
        process_path: &Path,
        args: I,
    ) -> Result<ProcMacroClient, std::io::Error>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let (thread, process) = ProcMacroProcessSrv::run(process_path, args)?;
        Ok(ProcMacroClient {
            kind: ProcMacroClientKind::Process { process: Arc::new(process), thread },
        })
    }

    pub fn dummy() -> ProcMacroClient {
        ProcMacroClient { kind: ProcMacroClientKind::Dummy }
    }

    pub fn by_dylib_path(
        &self,
        dylib_path: &Path,
    ) -> Vec<(SmolStr, Arc<dyn ra_tt::TokenExpander>)> {
        match &self.kind {
            ProcMacroClientKind::Dummy => vec![],
            ProcMacroClientKind::Process { process, .. } => {
                let macros = match process.find_proc_macros(dylib_path) {
                    Err(err) => {
                        eprintln!("Fail to find proc macro. Error: {:#?}", err);
                        return vec![];
                    }
                    Ok(macros) => macros,
                };

                macros
                    .into_iter()
                    .filter_map(|(name, kind)| {
                        // FIXME: Support custom derive only for now.
                        match kind {
                            ProcMacroKind::CustomDerive => {
                                let name = SmolStr::new(&name);
                                let expander: Arc<dyn ra_tt::TokenExpander> =
                                    Arc::new(ProcMacroProcessExpander {
                                        process: process.clone(),
                                        name: name.clone(),
                                        dylib_path: dylib_path.into(),
                                    });
                                Some((name, expander))
                            }
                            _ => None,
                        }
                    })
                    .collect()
            }
        }
    }
}
