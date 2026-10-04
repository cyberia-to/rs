# Rs toolchain execution plan

Active plan: [Rs development and verified bootstrap](../../roadmap/verified-bootstrap.md).
RS0–RS9 are open; their check identifiers and receipts are future work. This plan
supersedes the earlier locked rustc-frontend/sealed-stage0 architecture for the
accepted bootstrap route. The historical implementation plan remains useful for
the driver/lint compatibility track; it cannot establish rooted acceptance.

## Required route

Reviewed native nox seed and frozen Eidos root → admitted Trident route → bounded
Rs interpreter in Trident → own compiler C_B, written in restricted Rs B → native
Rs R1 → native self-build and full Rust-stack compilation. Initial seed/native
target: Linux AArch64; macOS seed construction needs its own review. Rs product
scope remains macOS and Linux. Trident's Windows gates are unaffected.

B covers the entire compiler/build/library/generator closure. L, the language
accepted by C_B, grows to the full pinned Rust compatibility contract. The
bootstrap interpreter executes B; no full Rust interpreter is required. Own
parsing, resolution, types/ownership, IR and native lowering replace rustc on the
accepted route. Maintain independent Rust and Trident implementations of every
critical operation, including the Rs compiler itself. Interpreting C_B on a
Trident interpreter alone does not supply that independent compiler counterpart.

Root replay excludes rustc, mrustc, LLVM and the old sealed compiler image as
trusted prerequisites. Root logic, directly reviewed images, native seed and
ISA/OS assumptions remain explicit. Sealing and self-reproduction alone cannot
establish source correspondence or compiler semantics.

## Existing compatibility track

`rsc/` currently uses installed `rustc_driver` for frontend/compilation and adds
Rs lint callbacks plus optional MIR export. `codegen/` is a separate rustc-private
plugin: in-process rustc MIR → local LIR → ARM64 → own Mach-O emitter/linker.
It uses a pinned nightly and copied Trident encoders. These paths can remain
development tools and differential oracles; they do not close RS root gates.
The default Cargo workspace excludes both `rsc` and `codegen`.

The previous code-complete labels, effort estimates and compatibility assertions
were planning checkpoints, not acceptance receipts for this new route. Preserve
the actual implementation and validate reuse through the RS operation inventory.

## Work retained from the native-toolchain plan

- Native backend: complete integer/float semantics, aggregates, calls, closures,
  dynamic dispatch, atomics, TLS and assembly where the accepted language needs
  them; define IR, ABI, register/stack layout and malformed-input rejection.
- Artifacts: own object/executable emission, relocations, symbol resolution,
  ELF/static Linux and Mach-O/macOS linking, loader contracts and code signing
  where required. External assemblers/linkers cannot fill accepted-route gaps.
- Runtime: core/alloc/std, allocator, compiler builtins, platform startup and
  syscall/FFI adapters. Preserve Darwin ABI/honeycrisp work and Linux syscall work
  where used. All required helpers need source and correctness obligations.
- Build tools: pinned source acquisition, dependency/features/workspace resolution,
  build graph, macros/generators and capability-scoped build scripts. Compact
  `rsc-build` coverage is an intermediate milestone; the full product/stack closure
  determines completion. Rebuild generated and prebuilt inputs through the root.
- Purity: eliminate C/C++ build tooling and dependencies in the admitted closure;
  vendor OS runtime remains a named environment boundary. Scanners and provenance
  checks supplement exact executable-input inventories and clean replay.
- Compatibility: full pinned Rust behavior remains a product obligation. Initial
  panic-abort and limited library/crate profiles are partial milestones. Unwinding,
  proc macros and other required semantics cannot be permanently waived by a pilot.
- Validation: fail closed on unsupported lowering, unresolved symbols and linker
  failure; retain code/ABI corpora, differential checks, exact artifact comparison,
  semantic certificates and independently replayed full self-builds.

## Next delivery

RS0: freeze B/L profiles, full compiler/build/dependency closure, Rust/Trident
operation matrix, host capabilities and platform claims. Then run a bounded
RS1–RS4 source-to-native experiment before sizing whole-compiler work. Use owning
feature branches and record evidence in `audit/verified-bootstrap/`. Root checking
is available before final paired Eidos/product acceptance; final RS delivery feeds
VB8. Every new source/profile/root version needs explicit identities and checks.
