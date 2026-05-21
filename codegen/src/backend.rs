use std::any::Any;
use std::os::unix::fs::PermissionsExt;

use rustc_codegen_ssa::traits::CodegenBackend;
use rustc_codegen_ssa::{CodegenResults, CrateInfo};
use rustc_data_structures::fx::FxIndexMap;
use rustc_metadata::EncodedMetadata;
use rustc_middle::dep_graph::{WorkProduct, WorkProductId};
use rustc_middle::ty::TyCtxt;
use rustc_session::Session;
use rustc_session::config::{OutFileName, OutputFilenames, OutputType};

use crate::codegen::TridentOutput;

fn find_macho_linker() -> Option<std::path::PathBuf> {
    if let Ok(s) = std::env::var("MACHO_LINKER") {
        let p = std::path::PathBuf::from(s);
        if p.exists() { return Some(p); }
    }
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let workspace = std::path::Path::new(manifest_dir).parent()?;
    for profile in &["debug", "release"] {
        // Workspace shared target (most common — macho-linker is a workspace member).
        let p = workspace.join("target").join(profile).join("macho-linker");
        if p.exists() { return Some(p); }
        // Crate-local target (when built standalone).
        let p = workspace.join("macho-linker/target").join(profile).join("macho-linker");
        if p.exists() { return Some(p); }
    }
    None
}

fn invoke_macho_linker(obj: &std::path::Path, exe: &std::path::Path) -> bool {
    let ml = match find_macho_linker() {
        Some(p) => p,
        None => return false,
    };
    match std::process::Command::new(&ml)
        .arg("-o").arg(exe)
        .arg(obj)
        .arg("-lSystem")
        .status()
    {
        Ok(s) if s.success() => true,
        Ok(s) => {
            eprintln!("warning: macho-linker exited with {:?}", s.code());
            false
        }
        Err(e) => {
            eprintln!("warning: could not run macho-linker: {e}");
            false
        }
    }
}

pub struct TridentBackend;

impl CodegenBackend for TridentBackend {
    fn locale_resource(&self) -> &'static str { "" }

    fn name(&self) -> &'static str { "trident" }

    fn codegen_crate<'tcx>(&self, tcx: TyCtxt<'tcx>) -> Box<dyn Any> {
        Box::new(crate::codegen::codegen_crate(tcx))
    }

    fn join_codegen(
        &self,
        ongoing_codegen: Box<dyn Any>,
        _sess: &Session,
        outputs: &OutputFilenames,
    ) -> (CodegenResults, FxIndexMap<WorkProductId, WorkProduct>) {
        let out = *ongoing_codegen
            .downcast::<TridentOutput>()
            .expect("TridentBackend::join_codegen: wrong type");

        let wants_obj = outputs.outputs.contains_key(&OutputType::Object);
        let wants_exe = outputs.outputs.contains_key(&OutputType::Exe);
        let using_tmp_obj;

        // Always write the .o file (needed for the linker path).
        let obj_path: std::path::PathBuf = if wants_obj {
            let obj_out = outputs.path(OutputType::Object);
            if let OutFileName::Real(ref p) = obj_out {
                std::fs::write(p, &out.object)
                    .unwrap_or_else(|e| panic!("write obj {}: {e}", p.display()));
                using_tmp_obj = false;
                p.clone()
            } else {
                using_tmp_obj = true;
                std::env::temp_dir().join(format!("trident_{}.o", std::process::id()))
            }
        } else {
            using_tmp_obj = true;
            std::env::temp_dir().join(format!("trident_{}.o", std::process::id()))
        };

        // Write to tmp path if we haven't written via the wants_obj branch.
        if using_tmp_obj {
            std::fs::write(&obj_path, &out.object)
                .unwrap_or_else(|e| panic!("write tmp obj: {e}"));
        }

        if wants_exe || !wants_obj {
            let exe_out = outputs.path(OutputType::Exe);
            if let OutFileName::Real(ref p) = exe_out {
                if !invoke_macho_linker(&obj_path, p) {
                    std::fs::write(p, &out.binary)
                        .unwrap_or_else(|e| panic!("write exe {}: {e}", p.display()));
                }
                std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755))
                    .unwrap_or_else(|e| panic!("chmod failed: {e}"));
            }
        }

        if using_tmp_obj {
            std::fs::remove_file(&obj_path).ok();
        }

        let cr = CodegenResults {
            modules: vec![],
            allocator_module: None,
            crate_info: out.crate_info,
        };
        (cr, FxIndexMap::default())
    }

    fn link(
        &self,
        _sess: &Session,
        _codegen_results: CodegenResults,
        _metadata: EncodedMetadata,
        _outputs: &OutputFilenames,
    ) {
        // Binary already written in join_codegen.
    }
}
