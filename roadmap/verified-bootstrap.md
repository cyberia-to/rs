---
status: accepted
milestone-status: open
---

# Rs development and verified bootstrap

The accepted development direction is an own-source Rs compiler bootstrapped
through Trident on the reviewed nox seed. All RS0–RS9 acceptance gates below are
open. Their identifiers name planned executable checks; runners and receipts
remain implementation work. The final product retains full Rust compatibility
as an obligation separate from the first restricted self-build.

The [soft3 ceremony](https://github.com/cyberia-to/soft3/blob/docs/verified-bootstrap-ceremony/docs/verified-bootstrap.md)
owns the cross-component trust and delivery contract.
[Trident VB0–VB8](https://github.com/cyberia-to/trident/blob/docs/0.4-dual-implementation-bootstrap/roadmap/verified-bootstrap.md)
owns its compiler/interpreter acceptance and the complete stack milestone. This
plan supplies Rs work packages to those contracts; finishing Rs alone cannot
close Eidos, nox, Zheng, Joy or their dependencies.

## Two language domains

Let **B** be the restricted Rs implementation language for the compiler and its
entire bootstrap build closure. Let **L** be the language that compiler accepts.
Initially L includes B; the product must grow L to the declared full Rust
compatibility contract. A compiler written in B can parse and compile features
outside B. The bootstrap interpreter needs to execute B, rather than full Rust.

RS0 freezes the exact B profile, Rust language/edition compatibility baseline,
target ABIs and the feature ledger extending L. Compatibility must name a pinned
language/library/tooling contract; later Rust evolution requires an explicit
upgrade. Restricting the compiler's own source does not excuse missing features
in L or the required native Rust stack.

B admission covers every transitive source actually executed to build the
compiler: libraries, build orchestration, macros, generators, generated sources,
runtime helpers and linker/emitter code. A dependency using an unsupported
feature must be ported into B or drive a reviewed B extension before acceptance.
An unverified generated file or prebuilt object cannot replace that work.

The initial profile should favor explicit types, finite data variants, bounded
arenas/handles, checked indexing, deterministic integer operations and iterative
control. Freeze actual grammar, moves/borrows, aliasing, lifetimes, destructors,
evaluation order, layout, modules and admitted generics before implementing it.
If raw pointers, unsafe primitives, const evaluation or macros are needed, their
semantics and complete implementation closure acquire named obligations. Existing
Rs lint opt-outs cannot enlarge the bootstrap profile implicitly.

Track successful-work bounds separately from enforced limits. Fuel exhaustion
is a resource failure, never a successful compile or proof. Bound source/import
sizes, parsing, type/ownership analysis, instantiation, allocation, generated code
and diagnostics. Deterministic behavior includes errors, ordering, codecs and
declared logical charges. Host time and peak memory are measured separately.

## Current implementation and reusable work

The existing [`rsc` driver](../rsc/src/main.rs) links installed `rustc_driver` and
adds Rs lints; it also has a serialized MIR export mode. The separate
[`codegen` plugin](../codegen/src/lib.rs) consumes rustc MIR in process, lowers it
through local LIR/ARM64 code and uses the local [`link`](../link/src/lib.rs) and
[`macho-linker`](../macho-linker/src/main.rs) implementations. Its encoder source
records copying from Trident; shared ancestry belongs in the trust inventory.
`rsc` and `codegen` are excluded from default workspace membership.

These are development and compatibility paths. They provide reusable algorithms
and differential observations; they do not supply an own Rust frontend or an
accepted root-derived build. The previous plan's sealed mrustc/LLVM stage0 route
is superseded for Verified Bootstrap. A signature or repeated self-build records
an artifact's identity without independently establishing its source semantics.

Preserve and finish native instruction selection, relocations, object/executable
emission, ELF/Mach-O linking, standard libraries, allocator/builtins, Darwin ABI
wrappers and build tooling where the target closure uses them. Reuse requires
profile admission and checked semantics. Lowering failures, unsupported assembly,
unresolved symbols and failed linking must reject the artifact; warning and
continuing or changing the selected linker silently cannot satisfy a gate.

The rooted route implements its own source parser, resolver, type/ownership
checker and lowering. It has no dependency on rustc internals or rustc-produced
MIR. Existing MIR adapters can remain optional compatibility tools. Own IRs and
their encoded forms need explicit contracts; sharing a Rust source language or
an IR name supplies no equivalence proof.

## Root and artifact chain

```text
reviewed native nox seed + frozen Eidos root
  -> admitted Trident compiler/interpreter route
  -> independently implemented B interpreter in Trident
  -> interpret compiler C_B with its complete build inputs
  -> native Rs artifact R1
  -> native Rs self-build and full Rust-stack compilation
```

The initial root target is Linux AArch64. The native seed, initial Eidos image,
logical rules and ISA/OS execution contract remain explicit reviewed foundational
assumptions in VB1. MacOS seed construction needs a separate root review. Rs
product delivery continues to cover macOS and Linux; each claimed architecture
needs evidence. This plan changes no Trident Windows support or distribution gate.

Root replay starts from retained seed bytes, source, proofs and explicit host
contracts. Rustc, mrustc, LLVM, C/C++ compilers, foreign proof assistants and old
sealed compiler binaries cannot be prerequisites for acceptance. Development
tools may propose sources, artifacts or proofs; the admitted route checks their
exact identities and claims before use. Declared vendor OS/loader services remain
environment assumptions. Build tools, semantic adapters and native runtime
algorithms require their own implementation obligations.

Let I_B be the admitted Trident interpreter of B, C_B the compiler source written
in B, and D the complete ordered build-input manifest. D includes C_B, every
dependency/generated source, build steps, flags, target, resource profile and
declared host responses. The first native bootstrap comparison is:

```text
R1 = I_B(C_B, D)
R2 = Run_native(R1, D)
compare_exact(R1, R2)
```

R1 is the compiler artifact produced by executing C_B's specified build through
I_B. R2 is produced by that compiler with the same source/build inputs. Retain
the complete output set for both invocations, including libraries, metadata and
build tools, with exact comparison of all behavior-affecting bytes. Any excluded
metadata needs a justified semantics-preserving rule. Bind the parent C_B and
target/configuration explicitly; changing parent source starts a new comparison.

This fixed point is one check. Acceptance additionally needs interpreter
refinement, source-to-native translation correctness and native execution/ABI
semantics for the exact artifacts. A compiler that reproduces a source-visible
bug can still reach a fixed point. Tests and agreement with rustc supplement
those obligations. Comparison, decoding, expected-statement selection and result
readback must use the admitted route rather than candidate-controlled helpers.

## Maintained implementation pairs

Every critical operation needs independently authored Rust and Trident paths:
B parsing/admission/interpreter, compiler frontend/analysis/lowering, native
emission/linking, bootstrap runtime, build graph/capability policy, codecs and
artifact publication. Place source with the component owner. B is implemented
within Rs/Rust; independently written Trident counterparts remain mandatory.
Simply interpreting the same C_B through I_B gives another execution route for
that algorithm, not an independently authored second compiler implementation.

RS0 records specification, both source paths, artifacts/lineage, dependency
closure, proof obligation, comparison rule and corpus per operation. Missing
paths or evidence keep RS8/RS9 and VB8 open. Shared encoders, generated code,
specifications and tests are disclosed as correlated evidence. Check both paths
against the reviewed semantics; deterministic outputs/errors and canonical bytes
must agree where the contract requires them. Different valid native layouts need
semantic comparison in addition to per-compiler reproducibility.

## Gates and acceptance dependencies

Acceptance prerequisites govern closing a gate. Implementations and proof
producers can be developed earlier without being trusted for acceptance. Root
checking of the initial Trident/B interpreter images must be possible before the
native Rs toolchain exists. It cannot require RS5, RS9 or final VB8. A sufficient
frozen Trident translation certificate and accepted nox/Eidos root profile may
establish that early route; later whole-product gates aggregate the full closure.
If a proof needs more Eidos rules, follow VB1's explicit root-extension policy.
Root logic/model availability is separate from final paired Eidos product
acceptance. RS8 uses the accepted root or separately reviewed root extension;
it does not depend on final Eidos E4 or VB8. Building native Eidos in RS7 is an
artifact-production step; its downstream E4 acceptance may require RS8. RS9
supplies replay evidence to final VB8, which cannot be its own prerequisite.

| Gate | Planned executable check | Acceptance prerequisites | Status |
|---|---|---|---|
| RS0 | `rs.scope`: B/L profiles, complete inventories and trust contracts | VB0 scope coordination | open |
| RS1 | `rs.interpreter`: B interpreter pair and admitted Trident image | RS0; accepted seed/Eidos and initial Trident image construction | open |
| RS2 | `rs.bootstrap-closure`: every executed bootstrap source admitted in B | RS0, RS1 admission contract | open |
| RS3 | `rs.frontend`: own B compiler frontend and semantic model | RS1, RS2 | open |
| RS4 | `rs.native`: native code, linking/runtime and exact artifact semantics | RS2, RS3; accepted root proof profile | open |
| RS5 | `rs.self-build`: root-derived R1/R2 with checked correspondence | RS1–RS4 | open |
| RS6 | `rs.rust-compatibility`: full pinned Rust input contract | RS5; each added feature checked through RS3/RS4 | open |
| RS7 | `rs.stack-closure`: actual full Rust stack and build ecosystem | RS5, RS6 | open |
| RS8 | `rs.cross-verification`: complete critical implementation pairs | RS1–RS7; required component proof profiles | open |
| RS9 | `rs.delivery`: independent replay and claimed native platforms | RS0–RS8; feeds final VB8 | open |

### RS0 — Freeze profiles and inventory

Capture the current driver/plugin path, the B implementation grammar, the L
compatibility ledger and native platform inventory. Name every source, artifact,
build step, generator, macro, library, host effect and native dependency. Distinguish
root assumptions, proof obligations and measured comparisons. Inventory rejection
checks cover omitted imports, injected binaries, changed flags/targets, undeclared
effects and missing implementation-pair rows. Existing broad compatibility claims
must be tied to the ledger's actual evidence rather than inherited as results.

### RS1 — Interpreter and admission pair

Implement independent B interpreters/admission checkers in Rust and Trident,
including source decoding, modules, name/type/ownership checks, memory/effects,
arithmetic, control and resource errors. Compare malformed and valid programs,
aliasing/move boundaries, arithmetic limits, import cycles and exhaustion cases.
Root-check refinement of the Trident interpreter's actual nox image to the frozen
B semantics. A byte-bound corpus covers the whole C_B build-language surface;
the interpreter must not silently delegate a construct to rustc or native Rust.

### RS2 — Entire bootstrap closure in B

Admit compiler sources, libraries, driver, build graph, format readers/writers,
linker and runtime helpers. Re-execute macros/generators from admitted sources and
compare resulting source bytes; handwritten fixed expansions need their own
reviewed source identity and must cease to be presented as an unverified generated
shortcut. Hidden `core`/prelude/lang-item/builtin behavior needs an explicit
definition. Enumerate permitted filesystem, process and environment operations;
bind outputs, prevent undeclared inputs, and preserve errors and atomic publication.

### RS3 — Own compiler implementation

Write C_B within B and its independent Trident counterpart. Implement its own
lexer/parser, module and crate resolution, type/ownership analysis, admitted
generics/const semantics and internal representations. Initially compile all B
programs in the frozen contract. Specify diagnostic/error classes and reject
unsupported source before emitting accepted artifacts. Evidence connects exact
source bytes to checked program meaning and subsequent lowering; a rustc-exported
MIR file cannot stand in for the frontend. The full L feature ledger remains open.

### RS4 — Native lowering, artifact construction and runtime

Specify native word/overflow semantics separately from Goldilocks arithmetic.
Cover instruction selection, register/stack layout, calls, memory, ABI, object
encoding, relocations, symbol resolution, executable format, allocator/builtins
and platform startup/exit. Begin with Linux AArch64, then cover every claimed
macOS/Linux product target. Reuse ARM64/Mach-O work only after checking its actual
source closure and semantics. Never substitute the old rustc plugin or an external
assembler/linker for an absent own implementation.

Require a checked preservation/refinement theorem for the admitted native domain,
or checked translation validation for every accepted artifact with that narrower
claim explicit. Include library/runtime/link steps and exact ISA/OS assumptions.
Test arithmetic, layout, calls, relocation corruption, malformed images, resource
failure and expected outputs. Measured execution and successful linking alone
cannot establish the semantic artifact claim.

### RS5 — Root-derived native self-build

Execute the R1/R2 construction above on the complete frozen compiler build closure.
Retain sources, compiler images, native outputs, claims/proofs, invocations and
comparison/readback identities. The reviewed route must bind actual native output
bytes, not just a hash supplied by the producer. Independently replay with the
old toolchain absent. Include a controlled contaminated seed/output fixture whose
ordinary self-build can reproduce but the independent source/artifact gate rejects.
Keep failures and source-visible bugs distinct from hidden binary substitution.

### RS6 — Full Rust compatibility

Extend L to the complete pinned contract: syntax/editions/modules, traits and
generics, ownership/lifetimes, closures and dynamic dispatch, const evaluation,
declarative/procedural macros, unsafe/FFI and layout, async, atomics/TLS, floats,
and runtime panic/unwind behavior where Rust requires them. Preserve ordinary
Rust behavior alongside explicitly selected Rs restrictions. C_B's implementation
must remain within B; expanding B requires RS0–RS5 evidence for the new closure.

Every supported feature has acceptance, rejection and semantic obligations;
upstream test corpora and differential compilation are additional evidence. A
panic-abort pilot, fork-per-test workaround or chosen crate corpus closes only
that pilot's scope. Full compatibility remains open while required Rust semantics
are absent. Unsupported future language versions must be identified explicitly.

### RS7 — Native Rust-stack and build closure

Build the actual frozen stack with the accepted own compiler: Rust Eidos, nox,
Zheng, Joy, Trident and all their native dependencies, plus Rs's own tools. Include
std/core/alloc/builtins, required allocators, proc macros, generated code, platform
runtime, linker and build driver. Inputs and intermediate artifacts have pinned
source provenance. Rebuilding compiler code alone cannot close this gate.

Implement all Cargo/workspace/features/lockfile/build-script behavior the declared
product contract and pinned stack require. The old compact build-driver subset is
an intermediate milestone. Native C/C++ build dependencies require replacement or
an explicit product-scope decision; they cannot enter a C-free accepted closure.
Vendor OS runtime remains a named boundary. Execute macros/build scripts under
declared capabilities with rooted compiler/runtime provenance; no hidden rustc,
foreign compiler or downloaded executable can fill a gap.

### RS8 — Whole-scope cross-verification

Close each implementation-pair row through root-checked refinement or complete
admitted-operation validation. Include compiler analysis, emitters/linkers, build
selection, codecs, host policy and publication. Test both execution paths with
valid, malformed, boundary and exhaustion inputs. Verify memory safety, resource
errors, deterministic behavior and secret handling where applicable. Shared
source/generated translations or common compiler ancestry require explicit
accounting; agreement alone cannot establish independence or correctness.

### RS9 — Independent native delivery

Replay bootstrap and full-stack construction from the reviewed seed and retained
files in a fresh environment without the old toolchain. Bind root assumptions,
source versions, proofs, all artifact bytes, comparison rules and platform results
into a delivery manifest. The macOS/Linux matrix is explicit per architecture;
an additional native seed target needs separate review. Missing compatibility,
implementation-pair or source/dependency evidence keeps this gate and VB8 open.
Owner-controlled release/tag/publication policy continues to apply.

## Delivery and evidence

Start with RS0 and a bounded real source-to-native vertical experiment through
RS1–RS4. Size subsequent slices in sessions/pomodoros from measured evidence;
the former rustc-plugin estimates do not size an own frontend or verified bootstrap.
Keep feature branches and owner-scoped PRs. Contracts belong in `reference/` (or
the owner's specification directory); receipts belong in `audit/verified-bootstrap/`.
Each receipt names commands, exact revisions, input/artifact identities, assumptions,
limits, outcome and original failures. Preserve the independent review of root,
semantic mappings and acceptance dependencies. No RS gate is closed by this plan.
