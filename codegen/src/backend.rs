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

        // Write the Mach-O binary to the output path.
        let exe_out = outputs.path(OutputType::Exe);
        let exe_path = match exe_out {
            OutFileName::Real(ref p) => p.clone(),
            OutFileName::Stdout => panic!("cannot write executable to stdout"),
        };
        std::fs::write(&exe_path, &out.binary)
            .unwrap_or_else(|e| panic!("failed to write {}: {e}", exe_path.display()));
        std::fs::set_permissions(&exe_path, std::fs::Permissions::from_mode(0o755))
            .unwrap_or_else(|e| panic!("chmod failed: {e}"));

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
