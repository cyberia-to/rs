//! rsc — Rs compiler driver.
//!
//! A clippy-style rustc driver that registers Rs edition lint passes.
//! Links against installed rustc libraries (via rustc-dev component).
//!
//! Usage:
//!   rsc my_program.rs                    # compile normally (attribute lints only)
//!   rsc --rs-edition my_program.rs       # compile with all Rs edition lints
//!   rsc --emit=mir-rs my_program.rs      # emit serialized MIR JSON (no codegen)
//!   rsc --explain RS206                  # explain an Rs error code
//!   rsc --rs-list-errors                 # list all Rs error codes

#![feature(rustc_private)]
#![feature(box_patterns)]

extern crate rustc_driver;
extern crate rustc_interface;
extern crate rustc_session;
extern crate rustc_lint;
extern crate rustc_span;
extern crate rustc_hir;
extern crate rustc_middle;
extern crate rustc_target;

mod emit_mir;
mod lints;

use std::env;
use std::process;

struct RsCallbacks {
    rs_edition: bool,
    emit_mir: bool,
}

impl rustc_driver::Callbacks for RsCallbacks {
    fn config(&mut self, config: &mut rustc_interface::interface::Config) {
        let rs_edition = self.rs_edition;
        let previous = config.register_lints.take();
        config.register_lints = Some(Box::new(move |sess, store| {
            if let Some(ref prev) = previous {
                prev(sess, store);
            }
            lints::register_all(store, rs_edition);
        }));
    }

    fn after_analysis(
        &mut self,
        _compiler: &rustc_interface::interface::Compiler,
        tcx: rustc_middle::ty::TyCtxt<'_>,
    ) -> rustc_driver::Compilation {
        if self.emit_mir {
            emit_mir::serialize_mir(tcx);
            return rustc_driver::Compilation::Stop;
        }
        rustc_driver::Compilation::Continue
    }
}

fn main() {
    let mut args: Vec<String> = env::args().collect();

    if args.iter().any(|a| a == "--rs-list-errors") {
        print!("{}", lints::rs_diag::list_all());
        return;
    }

    if let Some(pos) = args.iter().position(|a| a == "--explain") {
        if let Some(code) = args.get(pos + 1) {
            if code.starts_with("RS") {
                if let Some(explanation) = lints::rs_diag::explain(code) {
                    print!("{}", explanation);
                } else {
                    eprintln!("error: unknown Rs error code: {}", code);
                    process::exit(1);
                }
                return;
            }
        }
    }

    // Extract custom flags before passing args to rustc.
    let rs_edition = args.iter().any(|a| a == "--rs-edition");
    let emit_mir = args.iter().any(|a| a == "--emit=mir-rs");
    args.retain(|a| a != "--rs-edition" && a != "--emit=mir-rs");

    rustc_driver::install_ice_hook("https://github.com/nickcyber/rs/issues", |_| ());

    let exit_code = rustc_driver::catch_with_exit_code(|| {
        rustc_driver::run_compiler(&args, &mut RsCallbacks { rs_edition, emit_mir })
    });

    process::exit(exit_code as i32);
}
