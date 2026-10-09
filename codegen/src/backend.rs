use std::any::Any;
use std::os::unix::fs::PermissionsExt;

use rustc_codegen_ssa::back::archive::ArArchiveBuilderBuilder;
use rustc_codegen_ssa::back::link::link_binary;
use rustc_codegen_ssa::traits::CodegenBackend;
use rustc_codegen_ssa::{CodegenResults, CompiledModule, CrateInfo, ModuleKind};
use rustc_data_structures::fx::FxIndexMap;
use rustc_metadata::EncodedMetadata;
use rustc_middle::dep_graph::{WorkProduct, WorkProductId};
use rustc_middle::ty::TyCtxt;
use rustc_session::config::{OutFileName, OutputFilenames, OutputType};
use rustc_session::Session;

use crate::codegen::TridentOutput;

fn find_macho_linker() -> Option<std::path::PathBuf> {
    if let Ok(s) = std::env::var("MACHO_LINKER") {
        let p = std::path::PathBuf::from(s);
        if p.exists() {
            return Some(p);
        }
    }
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let workspace = std::path::Path::new(manifest_dir).parent()?;
    for profile in &["debug", "release"] {
        // Workspace shared target (most common — macho-linker is a workspace member).
        let p = workspace.join("target").join(profile).join("macho-linker");
        if p.exists() {
            return Some(p);
        }
        // Crate-local target (when built standalone).
        let p = workspace
            .join("macho-linker/target")
            .join(profile)
            .join("macho-linker");
        if p.exists() {
            return Some(p);
        }
    }
    None
}

fn invoke_macho_linker(
    obj: &std::path::Path,
    exe: &std::path::Path,
    rlibs: &[std::path::PathBuf],
) -> bool {
    let ml = match find_macho_linker() {
        Some(p) => p,
        None => return false,
    };
    let mut cmd = std::process::Command::new(&ml);
    cmd.arg("-o").arg(exe).arg(obj).arg("-lSystem");
    // Link the whole dependency closure (core, alloc, std, compiler_builtins, …)
    // from their sysroot rlibs. macho-linker reads archives whole-archive; rlib
    // symbol sets are disjoint, so link order only matters for rare duplicates.
    for rlib in rlibs {
        cmd.arg(rlib);
    }
    match cmd.status() {
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
    fn locale_resource(&self) -> &'static str {
        ""
    }

    fn name(&self) -> &'static str {
        "trident"
    }

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

        // Always materialize the .o file: the exe path links it here, the
        // rlib/staticlib path archives it in `link()`.
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
        } else if wants_exe {
            using_tmp_obj = true;
            std::env::temp_dir().join(format!("trident_{}.o", std::process::id()))
        } else {
            // Non-exe outputs (rlib / staticlib): keep the object alive for
            // `link()` — rustc's link_binary archives it from CodegenResults.
            using_tmp_obj = false;
            let p = outputs.temp_path_for_cgu(OutputType::Object, "trident", None);
            std::fs::write(&p, &out.object)
                .unwrap_or_else(|e| panic!("write obj {}: {e}", p.display()));
            p
        };

        // Write to tmp path if we haven't written via the wants_obj branch.
        if using_tmp_obj {
            std::fs::write(&obj_path, &out.object).unwrap_or_else(|e| panic!("write tmp obj: {e}"));
        }

        if wants_exe {
            let exe_out = outputs.path(OutputType::Exe);
            if let OutFileName::Real(ref p) = exe_out {
                // Dependency-closure rlibs (sysroot std/core/alloc/…) to link.
                let crate_info = &out.crate_info;
                let mut rlibs: Vec<std::path::PathBuf> = Vec::new();
                for &cnum in &crate_info.used_crates {
                    let src = match crate_info.used_crate_source.get(&cnum) {
                        Some(s) => s,
                        None => continue,
                    };
                    if let Some((ref rlib, _)) = src.rlib {
                        rlibs.push(rlib.clone());
                    }
                }
                let linked = invoke_macho_linker(&obj_path, p, &rlibs);
                if !linked && out.binary.is_empty() {
                    panic!(
                        "cannot produce {}: macho-linker unavailable and the monolithic \
                         emitter cannot bind external symbols (e.g. std vtable fn-ptrs)",
                        p.display()
                    );
                }
                if !linked {
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

        let modules = if wants_exe {
            vec![]
        } else {
            vec![CompiledModule {
                name: "trident".to_string(),
                kind: ModuleKind::Regular,
                object: Some(obj_path),
                dwarf_object: None,
                bytecode: None,
                assembly: None,
                llvm_ir: None,
                links_from_incr_cache: vec![],
            }]
        };

        let cr = CodegenResults {
            modules,
            allocator_module: None,
            crate_info: out.crate_info,
        };
        (cr, FxIndexMap::default())
    }

    fn link(
        &self,
        sess: &Session,
        codegen_results: CodegenResults,
        metadata: EncodedMetadata,
        outputs: &OutputFilenames,
    ) {
        use rustc_session::config::CrateType;
        // Executables were already linked by macho-linker in join_codegen.
        // Everything else (rlib, staticlib, standalone metadata) delegates to
        // rustc's link_binary with the pure-Rust ArArchiveBuilder — no LLVM,
        // no ranlib, no system linker on this path.
        let exe_pending = outputs.outputs.contains_key(&OutputType::Exe)
            && codegen_results
                .crate_info
                .crate_types
                .iter()
                .any(|c| *c == CrateType::Executable);
        if !exe_pending {
            link_binary(
                sess,
                &ArArchiveBuilderBuilder,
                codegen_results,
                metadata,
                outputs,
                self.name(),
            );
        }
    }
}
