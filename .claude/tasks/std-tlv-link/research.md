# research — std-binary runtime faults (exit hang + exit-code/stdout corruption)

serves: pure-rust-toolchain plan · rs/macho-linker+c codegen · std milestone deliverable
status: root causes found, fixed, verified; one downstream fault remains (btree do_merge)

## symptom

Any std binary (`fn main() { std::process::exit(42) }`, `println!`, std+itoa) hung:
the sample showed the main thread parked at a single in-__text leaf — a
fault → SIGSEGV handler → return → refault loop, not a spin.

## what it was (one chain, verified against dyld-1376.6 source + lldb)

1. `ARM64_RELOC_TLV_LOAD_PAGE21/PAGEOFF12` (8/9) skipped in macho-linker →
   thread-local descriptor addressing used placeholder page-relative values →
   `panic_count::increase` loaded code bytes as the descriptor pointer.
   ld64 semantics: the PAGEOFF12 site is `ldr Xt,[Xn,#off]` in the .o; when the
   descriptor is defined locally the linker rewrites it to `add` (new
   `arm64::ldr64_to_add`).
2. Mach-O section type bits were dropped (`sec_flags` synthesized 0/1/2):
   `__thread_vars` lost S_THREAD_LOCAL_VARIABLES (0x13), `__thread_bss` lost
   S_THREAD_LOCAL_ZEROFILL (0x12) and was not treated as zerofill (emitted size 0).
   Fix: preserve `flags & 0xFF` via `object::SectionFlags::MachO`.
3. `__thread_vars` claims 1-byte alignment in LLVM .o files; the LDR scaled
   immediate truncated the descriptor address to a lower 8-aligned slot. ld64
   forces 8-byte alignment. Fix: min align 3 for type-0x13 sections.
4. Section ordering: zerofill (`__bss`, `__common`, `__thread_bss`) sorted before
   file-backed sections in `__DATA`, so vm addresses diverged from file offsets
   by the zerofill VM extent. dyld maps a segment's file range onto its VM range
   1:1 → every file-backed section after the first zerofill was read from the
   wrong address (`__const`, descriptors, …). This was the value corruption
   ("exit 40 not 42, no stdout"). Fix: file-backed before zerofill per segment;
   `__thread_data`+`__thread_bss` adjacent (dyld accumulates the TLV initial
   content template over contiguous S_THREAD_LOCAL_* sections,
   `ThreadLocalVariables::findInitialContent`).
5. Descriptor finalization (new `tlv.rs`, runs after reloc application):
   .o shape `{__tlv_bootstrap, 0, $tlv$init_ptr}` → ld64 on-disk shape
   `{0, 0, template_offset}`. dyld validates offset ≤ template size and rewrites
   the thunk to v2 `{_tlv_get_addr, key, offset, delta, size}`
   (`initializeThunksFromDisk`).
6. `MH_HAS_TLV_DESCRIPTORS` (0x800000) was never set →
   `Loader::hasTLVs = mh.flags & MH_HAS_TLV_DESCRIPTORS` → dyld never ran
   `setUpThreadLocals` → thunks kept the `__tlv_bootstrap` error stub.

Verified at LC_MAIN entry (lldb, ASLR off): descriptor = v2 rewrite with
key=0x102, offset=0x28, size=0x91 — TLS works.

## commands used

- reproduce: nightly rustc `-Z codegen-backend=libcodegen.dylib -C panic=abort`
  on `fn main() { std::process::exit(42) }`, run under `lldb -b`, `sample`,
  `otool -tvV`/`xcrun llvm-objdump` for identification (shape-matched
  `panic_count::increase` against an LLVM build of the same program).
- dyld source: https://github.com/apple-oss-distributions/dyld tag dyld-1376.6 —
  `libdyld/ThreadLocalVariables.{h,cpp}`, `libdyld/threadLocalHelpers.s`,
  `dyld/Loader.cpp applyFixupsGeneric` (chained vs opcode fixups are mutually
  exclusive), `dyld/DyldRuntimeState.cpp` (hasTLVs gate).

## what I could not determine / remains

- The std binary now fails later: `alloc::collections::btree::node::
  BalancingContext<…ThreadInfo>::do_merge` (demanded in via
  `stack_overflow::thread_info`) makes an indirect call through a pointer into
  `__cstring` (faulting word 0x75722f00 = bytes of a path string). Call site
  0x100035b80 → target fn at 0x100003cf0; the bad pointer is not present in the
  file (no file offset contains 0x10008d3e8 outside a section header), so it is
  computed at runtime — suspect mir2lir lowering of do_merge or a data/code
  mixup in a vtable-like structure. Next task.
- ASLR note: all verification so far ran with slide 0 (lldb disables ASLR).
  Internal absolute pointers (vtables) are not in any fixup chain, so real
  (slid) runs likely still break — rebase coverage is a latent gap in
  macho-linker's chained-fixups output.

## session 3 addendum (committed 218419b, 8abf96c)

The btree crash was a chain of three independent bugs, found by watchpointing
the exact stack slot that carried a garbage formatter fnptr into
`core::fmt::write` (conditional watchpoint `$pc < 0x1000d0000`, dyld's own
stack writes filtered by pc-range — note `--stop-at-entry` stops at dyld's
entry, NOT LC_MAIN, and dyld's page-in linking re-maps text pages, which
silently deletes lldb software breakpoints):

1. `__rust_no_alloc_shim_is_unstable_v2` emitted as data, but the alloc rlib
   calls it → emitted as a one-`ret` function like rustc does.
2. GOT relocs on locally-defined symbols fell back to the direct address;
   `ldr [got]` then loaded instruction bytes as a pointer. Fixed with
   synthesized local GOT slots (absolute address content).
3. mir2lir call setup clobbered arg registers mid-move (lang_start's sigpipe
   got argv's low byte → `unreachable!()` in reset_sigpipe). Two-phase
   scratch-then-move; fat ptrs = 2 slots.

After these, std panics PRINT and startup passes sigpipe/TLS/alloc. The
remaining fault: `ldp [x10+0x20]`, x10=0x40, in btree node code reached via
`stack_overflow::thread_info` — next task.
