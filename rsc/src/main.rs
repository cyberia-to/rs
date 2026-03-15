//! rsc — Rs compiler driver.
//!
//! A clippy-style rustc driver that registers Rs edition lint passes.
//! Links against installed rustc libraries (via rustc-dev component).
//!
//! Usage:
//!   rsc my_program.rs                    # compile with Rs lints
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

mod lints;

use std::env;
use std::process;

struct RsCallbacks;

impl rustc_driver::Callbacks for RsCallbacks {
    fn config(&mut self, config: &mut rustc_interface::interface::Config) {
        let previous = config.register_lints.take();
        config.register_lints = Some(Box::new(move |sess, store| {
            if let Some(ref prev) = previous {
                prev(sess, store);
            }
            lints::register_all(store);
        }));
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();

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

    rustc_driver::install_ice_hook("https://github.com/nickcyber/rs/issues", |_| ());

    let exit_code = rustc_driver::catch_with_exit_code(|| {
        rustc_driver::run_compiler(&args, &mut RsCallbacks)
    });

    process::exit(exit_code as i32);
}
