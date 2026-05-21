# Pure Rust Toolchain — Implementation Plan

Status: architecture locked, post-audit. Ready to sequence.

## Goal

A Rust development stack with zero LLVM and zero C/C++ in any component the project ships or invokes as a build-time tool. Rust source → rustc frontend → Trident codegen → Rust linker → executable. Vendor-provided OS runtime is the only non-Rust code in the picture.

## What "Pure Rust" Means Here

The distinction that matters: **runtime vs build-time**.

**Runtime dependencies are fine.** The OS the user already runs is not part of our toolchain. The Linux kernel is C; macOS libSystem is C; Apple dyld is C++ — these are vendor-shipped components the user installed when they installed the OS. Calling them via FFI declarations (the way honeycrisp wraps IOSurface, IOKit, Metal) is no different from issuing syscall instructions on Linux. The OS boundary is the OS boundary.

**Build-time tools must be pure Rust.** Anything we invoke during compilation — compilers, linkers, assemblers, code generators, build scripts — is *our* infrastructure. Using Apple's `ld64` (C++) as our linker means our toolchain still ships C++ work. Using LLVM (C++) for codegen means the same. These are not OS components; they're tools we choose, and the choice must be Rust.

This is why the Mach-O linker is in scope, not deferred. `libSystem.dylib` is the OS; `ld64` is a C++ compiler tool. Pure-Rust on macOS requires replacing the latter, not the former.

By this definition:
- **In scope**: rustc frontend (already Rust), codegen, linker, loader (Linux), allocator, standard library, build scripts.
- **Out of scope (legitimately)**: kernel, libSystem.dylib, dyld, kernel32.dll, hardware microcode. These are OS or silicon.
- **Excluded targets**: Windows (per user decision); deferred until macOS + Linux are proven.

## Locked Architectural Decisions

These are the answers to the design questions worked out in the discussion. The rest of the plan flows from them.

1. **Trident is the only codegen backend.** No Cranelift, no LLVM, no codegen_gcc. Trident's `compile/` directory already has 14K LOC of native machine-code emitters; we reuse the encoding infrastructure and add LIR-driven instruction selection on top.

2. **Pipeline: rustc frontend → MIR (in-memory) → mir2lir (in-process) → LIR → native bytes → Mach-O / ELF → executable.** The frontend (parse, type-check, borrow-check, monomorphize, MIR) stays standard rustc; we plug in at the codegen interface as a `rustc_private` plugin like `rustc_codegen_cranelift` does. **No serialization anywhere — no JSON, no TOML, no binary IR format.** rustc gives us MIR via `TyCtxt`; we lower it in-process. MIR and LIR are both flat 3-address with virtual registers — they map 1:1 structurally. The cost of `rustc_private` coupling is version-pinning (we ship rustc + plugin as a pair); the benefit is no format design, no parsing overhead, and direct access to type information when needed. TIR remains the convergence IR for Trident's `.tri` source and for stack-based proof VMs (Triton, Miden, SP1); it is NOT on the native codegen path.

3. **nox stays frozen as the proof VM.** The existing `mir2nox.rs` (717 LOC in trident/) stays as the optional proof pathway for code that wants STARK proofs. The native codegen path does not go through nox. This preserves nox's 16-pattern proof property and avoids dragging tree-rewriting semantics into general Rust compilation. Same logic that excludes TIR from the native path: nox is structurally wrong (tree-rewriting), TIR is structurally wrong (stack-based) — both excellent in their domains, both inappropriate as a way station for flat 3-address MIR going to flat 3-address LIR.

4. **Parallel new LIR backends, with shared encoders.** Existing `compile/<isa>.rs` files (nox-driven, Goldilocks field arithmetic) stay untouched. New `compile/<isa>_lir.rs` files consume `&[LIROp]` and emit `Vec<u8>` for standard Rust u64 semantics. Both files import a shared `compile/<isa>_encoders.rs` module containing pure opcode emitters (~70 LOC per ISA). Confirmed by prototyping arm64.rs: only 27% of the existing file is reusable for LIR (the encoders); 43% is Goldilocks field reduction and 20% is nox-tree CF decoding — neither has an LIR analog. Refactor-in-place would fight the existing structure; parallel is cleaner and ~1,480 LOC total for three ISAs.

5. **Full standard Rust accepted.** The toolchain must compile arbitrary Rust crates (cargo, ripgrep, etc.), not just `rs`-subset code. This requires LIR extensions: atomics (~200 LOC LIR + ~300 LOC per backend lowering) and thread-local storage (~minimal on macOS — pthread_*, harder on Linux — segment registers). **Inline assembly is already a Tier 0 LIR op** (`LIROp::Asm { lines: Vec<String> }`), no LIR extension needed — only backend lowering for token-stream splicing.

6. **`panic = abort` for MVP.** Landing pads + DWARF CFI deferred to a later phase. Cargo test runs each `#[test]` in a forked process via `posix_spawn` until then. Document and accept the divergence from upstream cargo test behavior.

7. **macOS-first via honeycrisp.** aarch64-apple-darwin is the only target until the pipeline is proven end-to-end. Linux follows.

8. **Rs subset preferred for new code we write.** The `rs` lint suite (no heap, no dyn, bounded async, deterministic) applies to cyber-stack code. Standard Rust acceptance is for everything else.

9. **`rsc-build` replaces cargo, doesn't patch it.** Compact build tool, ~4,700 LOC, handles 80% of cases via Cargo.toml format for compatibility. Drops: alternative registries, complex feature unification, `[patch]`/`[replace]`, build.rs orchestration (refuses or runs explicitly with `--allow-build-script`), cargo publish/doc/bench/install, cross-compilation, multiple build profiles. Uses `toml`, `pubgrub`, `ureq`+`rustls`, `tar`, `miniz_oxide` — all pure Rust.

10. **Linux static-only by default.** No `ld.so` replacement. Every binary statically links libstd. Drops the entire 8K-LOC pure-Rust dynamic loader effort. Cost: larger binaries (~5MB vs ~500KB). Benefit: -8 sessions, reproducibility, attack-surface reduction. Dynamic linking can be added later if needed; the v1 toolchain doesn't require it.

11. **Mach-O linker scope: ~3K LOC, not 15K.** Uses the `object` crate (pure Rust) for Mach-O format I/O — saves most of what looked like format work. Phase 1 emits a monolithic single-binary Mach-O directly (~500 LOC, no multi-object linking). Phase 2 adds the full multi-object linker (~2,500 LOC additional) using the `object` crate writer, with our own symbol resolution / layout / relocation / dyld-binding / code-signing.

12. **compiler-builtins purification: focused, not exhaustive.** Port only the ~20 intrinsics actually invoked by typical Rust programs on aarch64 + x86_64. The remaining residue stays as upstream's asm/C until someone hits it. ~500 LOC, not 2K.

13. **Extended honeycrisp: scoped to std's minimum surface.** open/close/read/write, pthread_create/join, `__ulock_wait`/`__ulock_wake` for Mutex, getrandom, getenv/setenv, exit, fork+execve, getcwd, time. ~3K LOC of FFI declarations + safe wrappers. Not the full kqueue + mach ports + dispatch surface — those go in if/when needed.

## Pipeline (Locked)

```
Rust source
   │  rustc frontend (parse, typeck, borrowck, monomorphize)     existing, ~1M LOC, untouched
   ▼
MIR (post-monomorphization, concrete types only; flat CFG, 3-address)
                                                                  in-memory, via TyCtxt
   │  rustc_codegen_trident plugin                                NEW, ~2,000 LOC
   │  → mir2lir (in-process function call)                        NEW, ~2,000 LOC
   ▼
LIR  (53 ops, flat CF, 3-address, virtual regs; Asm already in Tier 0;
        extended with atomics + TLS in Tier 1)
   │  RegisterLowering trait                                      existing trait in trident/
   ▼
<isa>_lir.rs  +  <isa>_encoders.rs                                NEW, ~1,480 LOC total
   ▼  Vec<u8>  raw machine code
   │  Mach-O / ELF writer (object crate handles format I/O)       NEW glue, ~800 LOC each
   ▼
.o file  (or monolithic executable for single-binary case)
   │  Mach-O linker (NEW, macOS) / wild linker (ELF, Linux)       Mach-O ~3K LOC; wild existing
   ▼
executable (static-only on Linux; dyld-linked on macOS)
   │  dyld (vendor, macOS)                                        vendor-shipped, no ld-rs needed
   ▼  process runtime, linked against:
   │   - libSystem via honeycrisp-extended FFI (macOS)            ~3,000 LOC FFI declarations
   │   - rustix raw syscalls (Linux, static)                      existing
   │   - talc allocator                                           existing
   │   - libstd with macOS or Linux backend                       existing core, alloc, std + our backends
```

Why MIR → LIR direct, not MIR → TIR → LIR:

- MIR is flat-CFG + 3-address + virtual locals.
- LIR is flat-CF + 3-address + virtual `Reg(u32)`.
- TIR is stack-based + structural CF (`IfElse`/`IfOnly`/`Loop` nested bodies).
- Routing MIR through TIR would lift flat → stack-structural, then immediately lower stack-structural → flat in TIR → LIR. The structure MIR already has gets discarded and rebuilt. The `tir_to_lir` converter in `src/ir/lir/convert.rs` is currently `todo!()` — going direct from MIR avoids depending on it.
- TIR stays the right IR for: Trident's own `.tri` source compilation, stack VM proof targets (Triton, Miden, SP1) via `StackLowering`, GPU targets via `KIR`.

Optional side-channels (orthogonal to native codegen):

```
Proofs:           MIR → mir2nox (existing, 717 LOC) → nox formula → zheng prover → STARK proof
Stack VMs:        TIR → StackLowering → assembly text (Triton, Miden, SP1)
GPU kernels:      TIR → KIR → KernelLowering → PTX / MSL / SPIR-V
Trident source:   .tri → AST → TIR (this is what TIR was designed for)
```

Programs opt into proofs by compiling through `mir2nox` and shipping the proof alongside the native binary. The native binary itself comes from the mir2lir path; proofs are an additive artifact.

## Trident's Role — Verified

Trident's IR layering (`src/ir/`) gives us the right entry point for a Rust codegen backend. The full layering Trident maintains is:

```
                          ┌──→ StackLowering   → assembly  for stack VMs (Triton, Miden, SP1)
AST (Trident source) ─→ TIR──→ KIR → KernelLow → GPU code  for PTX, Metal, SPIR-V
                          └──→ TreeLowering    → nox       for nox/Nock tree VMs

                                  LIR ←─→ RegisterLowering → bytes  for x86_64, arm64, rv64
```

LIR is reached two ways: from TIR (via the `tir_to_lir` converter, currently `todo!()`), or directly from any flat-3-address input. **For rs's MIR-driven codegen we enter LIR directly via `mir2lir`**, because MIR is already flat-3-address — going through TIR would lift to stack form and immediately lower back.

The `RegisterLowering` trait exists today in `src/ir/lir/lower/mod.rs`. LIR has 53 operations covering everything needed for general Rust codegen — including `Asm` passthrough at Tier 0, full arithmetic / I/O / memory / hash / events / storage at Tier 1. Tiers 2-3 (proof-specific) are unused by the native path. The `mir-format` crate in `rs/` already serializes MIR to JSON. `mir2nox` (717 LOC) proves the MIR consumer pattern works.

We're not inventing architecture — we're extending the LIR consumer path. Trident's own source ingestion uses TIR; rs's MIR ingestion uses LIR. Both converge at LIR for register-machine native codegen.

**Existing Trident inventory we reuse**:
- IR infrastructure (TIR, LIR, KIR, Tree): ~10,000 LOC across `src/ir/`
- TIR optimizer + builder + encoder: existing
- RegisterLowering trait: ~30 LOC
- Existing nox-driven backends (x86_64, arm64, rv64, rv32, rvv, thumb2, wasm, ebpf, ptx, etc.): 14,331 LOC — untouched, reused only for their encoding tables via the shared `<isa>_encoders` modules
- Cost models, verifier, package store, deploy infrastructure: existing

**What's new** for the LIR codegen path:
- `<isa>_encoders.rs` per ISA — extracted pure encoders (~70 LOC each)
- `<isa>_lir.rs` per ISA — LIR consumer + instruction selection (~400 LOC each)
- mir2tir — the MIR → TIR bridge (~4,000 LOC)
- TIR extensions for atomics / asm / TLS (~500 LOC in TIR + lowerings)

## Component Inventory

### Embed (already pure Rust, production-grade)

| Component | Crate / project | Coverage |
|---|---|---|
| rustc frontend (parser, HIR, type-check, borrow-check, MIR, monomorphization) | rust-lang/rust | universal — untouched |
| std core, alloc | rust-lang/rust | universal |
| libm (pure Rust math) | rust-lang/compiler-builtins/libm | universal |
| Linux syscalls | rustix linux_raw backend | x86_64, aarch64, riscv64, others |
| Apple-framework FFI surface (compute) | honeycrisp (`~/cyber/honeycrisp/`) | IOSurface, IOKit, CoreFoundation, Metal, libobjc, AMX, NEON |
| Trident IR infrastructure | `~/cyber/trident/src/ir/` + `src/compile/` | TIR, LIR, KIR, native emitters (encoding tables) |
| Cryptography | RustCrypto suite | universal |
| TLS | rustls | universal |
| Networking | hyper, tokio, mio, h2 | universal |
| Allocator | talc | no_std, embedded, WASM, general |
| Compression | miniz_oxide, zstd-rs (pure mode), lz4_flex | universal |
| Hashing | hashbrown (SwissTable), ahash | universal |
| Serialization | serde | universal |
| Regex | regex | universal |
| Date/time | jiff | universal |
| Linker (ELF, Linux) | wild | x86_64-linux, aarch64-linux, riscv64-linux |
| Git operations | gitoxide | replaces libgit2 |
| HTTP client | reqwest + rustls | replaces libcurl |
| SSH | russh | replaces libssh2 |
| WASM runtime | wasmtime (with Cranelift removed) or wasmi | for WASM execution if needed |

### Create (new code, with LOC)

| Component | LOC | What it does |
|---|---:|---|
| mir2lir | 2,000 | Rust MIR (in-memory via TyCtxt) → LIR ops. Direct mapping — both are flat 3-address with virtual registers. In-process function call, no serialization. |
| `<isa>_lir.rs` (arm64, x86_64, rv64) | 1,250 | LIR consumers: instruction selection from `&[LIROp]` to native bytes |
| `<isa>_encoders.rs` shared modules | 230 | Pure opcode emitters extracted from existing `compile/<isa>.rs` |
| rustc_codegen_trident plugin | 2,000 | Hooks Trident into rustc's `codegen_ssa` interface as `rustc_private` plugin |
| LIR extensions (atomics, TLS) | 300 | New Tier 1 LIR ops. Asm is already in Tier 0, no extension needed. |
| Per-backend LIR-extension lowerings | 900 | ~300 LOC per backend for atomics, TLS, and Asm splicing |
| ELF object writer (using `object` crate) | 800 | Glue around the `object` crate's ELF writer — symbols, relocs, sections |
| Mach-O object writer (using `object` crate) | 800 | Same pattern, Mach-O format. The `object` crate handles the binary format I/O. |
| Mach-O linker (Phase 1 monolithic + Phase 2 multi-object) | 3,000 | Pure-Rust replacement for Apple ld64. Uses `object` crate for format I/O; our own symbol resolution, layout, relocation processing, dyld binding (eager only), ad-hoc code signing. In scope because ld64 is C++ build-time tooling, not OS runtime. |
| Extended honeycrisp / Darwin ABI surface | 3,000 | std-minimum FFI surface: open/close/read/write, pthread, ulock for Mutex, getrandom, getenv/setenv, exit, fork+execve, getcwd, time. Pattern from honeycrisp. |
| compiler-builtins focused purification | 500 | Port only the ~20 intrinsics actually invoked by typical Rust programs on aarch64 + x86_64. The remaining residue stays as upstream's asm/C. |
| `rsc-build` (compact cargo replacement) | 4,700 | Build tool: build/run/test/check, Cargo.toml-compatible, uses pubgrub/toml/ureq/tar. Drops registries, complex features, build.rs orchestration, publish/doc/bench/install, cross-compilation. |
| Sealed bootstrap binary + provenance bundle | 500 | One-time build + signed cache. Replaces mrustc dependency going forward. |
| `pure-rust-check` audit tool | 2,000 | Scans Cargo.lock + build artifacts, reports any non-Rust input |
| `build.rs` scanner | 1,000 | Detects when crates shell out to `cc` / `bindgen` / non-Rust toolchain |
| CI + reproducibility verifier | 500 | GitHub Actions + reproducibility check |

**Total new code: 23,480 LOC** — well inside the 50K envelope after audit. (Down from 47,680 pre-audit: -8K from dropping ld-rs, -12K from Mach-O linker right-sizing, -5K from rsc-build aggressive scope, -2K from extended honeycrisp scope, -1.5K from focused compiler-builtins purge, -1K each from mir2lir / rustc_codegen_trident leaner estimates, -1.4K from ELF/Mach-O writers using object crate.)

### Why Mach-O Linker Is In Scope

I originally marked it deferred. That was wrong. The reasoning:

Apple's `ld64` is a C++ build-time tool, not an OS runtime component. The user installs it as part of the Xcode Command Line Tools — separate from the macOS itself. A "pure Rust toolchain" that invokes `ld64` is shipping C++ work in its critical path; the toolchain is not actually pure Rust.

The runtime-vs-build-time distinction is what makes libSystem acceptable (it's the OS; user has it; we declare FFI) and ld64 unacceptable (it's a build tool; we invoke it; replacing it is our job).

This parallels the Linux situation exactly: wild replaces the GNU `ld` (C/C++ build tool) for the same reason. On Linux we don't get to say "well GNU ld is everywhere, let's just use it." We replace it because it's our linker, not the OS's. Same logic for Mach-O.

Concretely, the Mach-O linker is the difference between:
- ✓ "rustc → Trident codegen → Mach-O object writer → **Rust Mach-O linker** → Mach-O executable → dyld (vendor) → libSystem (vendor)"
- ✗ "rustc → Trident codegen → Mach-O object writer → **ld64 (Apple C++)** → Mach-O executable → dyld → libSystem"

The first is a pure-Rust toolchain on macOS. The second isn't. The Mach-O linker is on the critical path; deferring it means the entire macOS phase produces a non-pure toolchain. That defeats the project goal.

**Effort**: 15,000 LOC is realistic. Mach-O is well-documented (Apple publishes the format spec via xnu source). The work is: format reading/writing, segment/section layout, indirect symbol tables, weak/lazy binding, code signing (ad-hoc signature — SHA-256 over LC_CODE_SIGNATURE), dyld stub generation, LC_BUILD_VERSION setup. wild's object-file infrastructure is forkable (~5K of wild's code maps over to Mach-O concepts); the rest is Mach-O-specific.

### Out of scope

| Component | Why |
|---|---|
| Windows toolchain | User decision — macOS + Linux first |
| Replacing libSystem.dylib | It's the OS, not the toolchain |
| Replacing dyld | Same — vendor-shipped OS component, not a build tool |
| Replacing Linux kernel | OS, not toolchain |
| Pure-Rust source bootstrap (rewriting mrustc in Rust) | Multi-year project; sealed-binary approach makes it unnecessary |
| `panic = unwind` support (landing pads, DWARF CFI) | Deferred per Decision 3b; `panic = abort` for MVP |
| Native `cargo test` semantics with `catch_unwind` | Same — fork-per-test workaround until landing pads land |
| LLVM as fallback | Decision 1 — Trident only |
| Cranelift as fallback | Decision 1 — Trident only |

## Phases

Each phase produces a usable artifact. Estimates use 1 session = 3 focused hours.

### Phase 0 — Inventory and gating (2 sessions)

- Catalog every direct + transitive C/C++ dependency of cargo + rustc on macOS aarch64.
- Write `pure-rust-check`: scans Cargo.lock + build artifacts, reports non-Rust inputs.
- Define the gate: "the toolchain compiles itself + hello-world + ripgrep with zero non-Rust source."

**Deliverable**: `pure-rust-check` crate, baseline report.

### Phase 1 — macOS aarch64 MVP via Trident + honeycrisp (9 sessions)

Goal: hello-world compiled by rustc with zero LLVM, linked by a pure-Rust Mach-O writer, runs on Apple Silicon.

1. Extract `arm64_encoders.rs` from existing `compile/arm64.rs`. ~70 LOC. (1 session)
2. Write `arm64_lir.rs` — LIR consumer for arm64. ~400 LOC. (2 sessions)
3. Write `mir2lir` MVP subset — enough LIR ops to handle hello-world (integer arithmetic, Branch/Jump, Call/Return, Memory ops for the print buffer, Asm for the syscall stub). ~800 LOC. (2 sessions)
4. Write `rustc_codegen_trident` plugin skeleton — `rustc_private` integration, drives mir2lir → arm64_lir in-process. ~1,000 LOC. (2 sessions)
5. Monolithic Mach-O emitter — single binary directly from Trident output, no .o intermediate, no multi-object linker. Uses `object` crate for the Mach-O writer. ~500 LOC. (1 session)
6. Glue: minimal honeycrisp surface for `write(1, buf, n)` + `exit(0)`. ~200 LOC. (1 session)

**Deliverable**: `pure-rs-toolchain` directory with a Trident-backed rustc that produces a Mach-O executable running on aarch64-apple-darwin, printing "Hello, world!". No LLVM, no ld64, no clang. Verified with `pure-rust-check`.

### Phase 2 — Full Mach-O linker (5 sessions, ~2,500 LOC of the 3K total)

Build out the multi-object Mach-O linker for real workloads. Uses `object` crate for format I/O; our work is symbol resolution, layout, relocs, dyld binding, code signing.

1. Multi-object linking — symbol resolution + section layout + relocation application (ARM64_RELOC_*). (2 sessions)
2. dyld binding opcodes (eager only, LC_DYLD_INFO_ONLY bind stream). (1 session)
3. Code signing (ad-hoc SHA-256 super-blob; format documented in Apple xnu source). (1 session)
4. LC_BUILD_VERSION, LC_MAIN, LC_LOAD_DYLIB, LC_SYMTAB, LC_DYSYMTAB plumbing. Compile ripgrep end-to-end — verify with `pure-rust-check`. (1 session)

**Deliverable**: A pure-Rust Mach-O linker handling real-world dependency graphs. Ripgrep compiles and runs.

### Phase 3 — Standard Rust feature coverage (8 sessions)

Make the toolchain handle arbitrary Rust crates, not just hello-world shape.

1. Atomics: new LIR Tier 1 ops + arm64_lir lowering for load/store/swap/cas/fetch-add/fetch-sub. (2 sessions)
2. Inline asm: arm64_lir token-stream splicing for the existing `LIROp::Asm` (LIR side needs no extension). (1 session)
3. TLS via libSystem pthread_getspecific. (1 session)
4. Drop glue, `dyn Trait` vtable dispatch, closures. (2 sessions)
5. Full mir2lir coverage — all MIR rvalue kinds, all terminators, panic=abort lowering. (2 sessions)

**Deliverable**: cargo + ripgrep + serde + tokio compile and run.

### Phase 4 — Extended honeycrisp / Darwin ABI surface (3 sessions)

Extend honeycrisp from compute frameworks to the minimum surface std needs. Scoped tight — no kqueue / mach ports / dispatch unless a real workload actually needs them.

1. Process + filesystem: fork, posix_spawn, exit, waitpid, open, close, read, write, stat, mkdir, unlink, lseek. (1 session)
2. Threading + sync: pthread_create/join, `__ulock_wait` / `__ulock_wake` for Mutex (post-Sonoma) with `psynch_*` fallback. (1 session)
3. Misc: getrandom, getenv/setenv, getcwd, time. (1 session)

**Deliverable**: std macOS backend built entirely on honeycrisp-extended FFI. The `libc` crate is no longer a dependency of std for the surface std needs.

### Phase 5 — compiler-builtins focused purification (2 sessions)

Port only the intrinsics actually invoked by typical Rust programs on aarch64 + x86_64 (about 20 of them). Remaining residue stays as upstream's asm/C until a real workload hits one.

1. Audit which intrinsics our representative workloads (hello-world, ripgrep, tokio, serde, hyper, rsc-build itself) actually call. Port those. (1 session)
2. Verify with `pure-rust-check` that no compiler-builtins C/asm is reached by the audited workloads. (1 session)

**Deliverable**: compiler-builtins in our build artifacts contains no C/asm code for the workloads we test against.

### Phase 6 — `rsc-build` (compact cargo replacement, 6 sessions)

Write a lean build tool that handles the cyber stack's needs without dragging in cargo's 150K LOC of accumulated features. ~4,700 LOC, uses pubgrub for resolution and the toml/ureq/tar/miniz_oxide crates for everything else.

1. CLI + Cargo.toml reader + lock file r/w. ~1,000 LOC. (1 session)
2. Dependency resolution via `pubgrub`; crates.io index reader + ureq+rustls fetch + tar.gz extract. ~1,300 LOC. (1 session)
3. Build graph computation + parallel rustc spawning + output capture. ~1,100 LOC. (1 session)
4. Test runner (fork-per-test); workspace traversal (basic single-level). ~800 LOC. (1 session)
5. Error reporting, lock conflict resolution, cache directory management. ~500 LOC. (1 session)
6. Verify with `pure-rust-check`: rsc-build's build artifacts and self-build are 100% pure Rust. (1 session)

What it explicitly doesn't do: cargo publish/doc/bench/install/vendor/package, alternative registries, `[patch]`/`[replace]`, build.rs orchestration (refuses or runs with `--allow-build-script`), cross-compilation, multiple profiles beyond dev/release, complex feature unification edge cases.

**Deliverable**: `rsc-build` binary, ~4,700 LOC, replaces cargo for the cyber stack. Existing crates with build.rs are rejected unless explicitly opted in.

### Phase 7 — Sealed bootstrap (4 sessions)

Build the pure-Rust rustc once (using mrustc + LLVM for the initial seed), sign and cache it as `stage0`. All future builds bootstrap from this cached binary.

1. Audit reproducibility of the build. (2 sessions)
2. Define seal format + signing keys + attestation chain. (1 session)
3. Publish stage0 with provenance. (1 session)

**Deliverable**: `pure-rust-rustc-stage0` — sealed binary + provenance bundle. mrustc dependency drops out of the chain.

### Phase 8 — Linux follow-on, static-only (5 sessions)

Bring up Linux as a second target. Static-only — no dynamic linker. Every binary statically links libstd; no `/lib/ld-linux.so` needed.

1. ELF object writer (using `object` crate) + wild integration. ~800 LOC. (1 session)
2. x86_64_encoders.rs + x86_64_lir.rs. ~600 LOC. (2 sessions)
3. rv64_encoders.rs + rv64_lir.rs. ~410 LOC. (1 session)
4. std Linux backend via rustix (already pure Rust — wiring + static-link config). (1 session)

**Deliverable**: Toolchain produces static ELF for x86_64-linux, aarch64-linux, riscv64-linux. No dynamic linking, no ld-rs. Dynamic support deferred until a real use case demands it.

### Phase 9 — Audit, CI, certification (4 sessions)

1. CI: `pure-rust-check` runs on every commit; fails on non-Rust input. (1 session)
2. Reproducibility: identical bytes across machines. (2 sessions)
3. Public dashboard: % of popular crates that build clean. (1 session)

**Deliverable**: Continuous proof that the toolchain is and stays pure.

## Total Estimate

| Phase | Sessions | LOC delivered |
|---|---:|---:|
| 0 — Inventory + audit tooling | 2 | 3,500 |
| 1 — macOS aarch64 MVP | 9 | ~3,200 |
| 2 — Full Mach-O linker | 5 | ~2,500 |
| 3 — Standard Rust coverage | 8 | ~2,200 |
| 4 — Extended Darwin ABI | 3 | 3,000 |
| 5 — compiler-builtins focused purge | 2 | 500 |
| 6 — `rsc-build` | 6 | 4,700 |
| 7 — Sealed bootstrap | 4 | 500 |
| 8 — Linux follow-on (static-only) | 5 | 1,800 |
| 9 — Audit + CI | 4 | 500 |
| **Total** | **48 sessions** | **~22,400 LOC new + ~25,000 LOC reused from trident/honeycrisp** |

48 sessions = ~144 focused hours. At one session/day: ~10 weeks solo. With phases 5, 6, 8 parallelizable to a second engineer: ~7 weeks on the critical path.

**Critical path to macOS pure-Rust toolchain**: Phases 0 → 1 → 2 → 3 → 4 = 27 sessions, ~5.5 weeks. After Phase 4, macOS is a pure-Rust target end-to-end. Phases 5-7 are cleanup; Phase 8 is Linux expansion; Phase 9 is continuous.

**Critical path to first lit pixel** (hello-world via Trident plugin, no LLVM, single Mach-O executable): Phase 1 alone = 9 sessions, ~2 weeks. This is the milestone that proves the architecture works before committing to the rest.

**Post-audit reduction**: original 71 sessions / 44,400 LOC → 48 sessions / 22,400 LOC. The audit cut:
- ld-rs entirely (−8K LOC, −5 sessions) — static-only Linux
- Mach-O linker right-sized using `object` crate (−12K LOC, −3 sessions)
- rsc-build vs cargo patching trade (−1.5K patch, +4.7K rewrite = net +3.2K but cleaner)
- mir2lir / rustc_codegen_trident leaner (−2K LOC)
- compiler-builtins focused on used-only (−1.5K LOC, −2 sessions)
- extended honeycrisp scoped (−2K LOC, −1 session)
- ELF/Mach-O writers using object crate (−2.4K LOC)
- No MIR serialization (plugin model) (−460 LOC, −1 session)

## Hard Blockers and Open Questions

1. **Mach-O linker correctness.** ~3K LOC after audit. Architecture is sound (uses `object` crate for format I/O, our work is symbol resolution + relocs + dyld binding + code signing). Mitigation: Phase 1 ships a 500-LOC monolithic single-binary Mach-O emitter that doesn't need multi-object linking — proves the format works before committing to Phase 2's full linker. Differential test against `ld64` output in Phase 2.

2. **LIR coverage of MIR semantics.** mir2lir is 2K LOC of estimate post-audit. Most MIR constructs map naturally because both IRs are flat 3-address. Harder cases: drop glue across panic-edges, vtables for `dyn Trait`, async state machines (already lowered by rustc, but resulting MIR is large). The estimate may need +50% buffer once Phase 3 begins; the audit can't know how many MIR constructs we'll discover.

3. **rustc_private brittleness.** Plugin model couples us to a specific rustc version. Each upstream rustc release may break compatibility. Mitigation: pin rustc version, ship the rustc+plugin pair as one unit, gate upgrades through deliberate validation. Same cost cranelift backend pays.

4. **Apple framework deprecation.** libSystem ABI is stable but Apple deprecates and occasionally removes calls. Honeycrisp-extended needs a per-macOS-version compatibility matrix. Adds maintenance burden, not blocked.

5. **Static-only Linux limitations.** Some crates assume `dlopen` exists (e.g., Wayland clients, plugin systems). These won't work under static-only. Mitigation: document the constraint loudly; add `ld-rs` later if a real workload needs it (the ~8K-LOC effort is shelved, not lost).

6. **Performance.** Trident's existing arm64.rs has not been benchmarked against LLVM for general Rust code (it's been validated for Goldilocks field arithmetic only). The LIR backends are new code. Expect 2-5× slower than LLVM-built binaries initially. Acceptable for a v1 toolchain; optimization is a separate workstream.

7. **panic = abort divergence from upstream cargo test.** Fork-per-test workaround handles the test-isolation case but doesn't replicate `catch_unwind` semantics for code that legitimately uses it. Document, accept, revisit when adding landing pads.

8. **`rsc-build` ecosystem fit.** ~4.7K LOC is aggressive. Some crates may use Cargo.toml features rsc-build doesn't implement (e.g., `[patch]` for local overrides, custom build profiles). Mitigation: rsc-build refuses unsupported features with clear errors; documents the supported subset; users requiring full cargo can use upstream cargo for their non-cyber-stack work.

9. **`build.rs` escape hatch.** rsc-build refuses `build.rs` by default; opt-in via `--allow-build-script`. Some legitimate crates use build.rs for non-C reasons (e.g., generating Rust code from .proto files). The policy is conservative — reject first, allow with explicit opt-in.

## What I'd Build First

Phase 0 + Phase 1: **11 sessions, ~2.5 weeks** to "Rust hello-world compiled by rustc with zero LLVM, runs on Apple Silicon." That milestone validates:
- Trident as a viable rustc backend (Decision 1)
- mir2lir as a workable bridge in-process via rustc plugin (Decisions 2 + 9)
- Parallel new LIR backends as the right architecture (Decision 4 from prototype)
- `object` crate carries Mach-O format I/O successfully (Decision 11 scoping)
- macOS-first focus is correct (Decision 7)
- honeycrisp pattern scales to std-required surface (Decision 13)

If any of those don't hold after Phase 1, replan. The plan beyond Phase 1 is provisional.

## References (verified May 2026)

- Trident IR: `~/cyber/trident/src/ir/` — TIR (`tir/mod.rs`, 431 LOC), LIR (`lir/mod.rs`, 398 LOC), KIR, Tree
- Trident backends: `~/cyber/trident/src/compile/` — 14,331 LOC, including x86_64.rs (582), arm64.rs (612), rv64.rs (583), mir2nox.rs (717), etc.
- mir-format crate: `~/cyber/rs/mir-format/` — 460 LOC, Rust MIR → JSON
- honeycrisp: `~/cyber/honeycrisp/` — Apple Silicon hardware drivers, established FFI pattern
- rustix: https://github.com/bytecodealliance/rustix — Linux raw syscalls
- wild linker: https://github.com/davidlattimore/wild — ELF, Linux only
- talc allocator: https://github.com/SFBdragon/talc — pure Rust, no_std
- gitoxide: https://github.com/Byron/gitoxide — replaces libgit2
- rustls: https://github.com/rustls/rustls — replaces openssl for TLS
- compiler-builtins: https://github.com/rust-lang/compiler-builtins — 93.3% Rust, 3.3% asm, 0.2% C
- mrustc (current bootstrap): https://github.com/thepowersgang/mrustc — used once to build stage0, then retired
- Mach-O format spec: Apple xnu source (`/EXTERNAL_HEADERS/mach-o/loader.h`)
