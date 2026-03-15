# Compiling a project with rsc

How to set up any Rust project to compile under the rs edition compiler.

## Prerequisites

Build rsc from the rs repo:

```nu
cd ~/git/rs/rsc
cargo build --release
```

The binary is at `~/git/rs/rsc/target/release/rsc`.

## Building your project with rsc

```nu
$env.RUSTC = ($env.HOME + "/git/rs/rsc/target/release/rsc")
$env.RUSTFLAGS = "--rs-edition"
cd ~/git/your-project
cargo build
```

Without `--rs-edition`, only attribute-triggered lints run (`#[deterministic]`, `#[step]`, `#[derive(Addressed)]`). With `--rs-edition`, edition restriction lints (RS501–RS507) activate.

## Edition restrictions (RS501–RS507)

Every violation must be fixed — not suppressed — unless interfacing with an external Rust crate.

| code | forbidden | replacement |
|------|-----------|-------------|
| RS501 | `Box::new()` | stack values or `Arena<T, N>` |
| RS502 | `Vec<T>` | `BoundedVec<T, N>` with compile-time capacity |
| RS503 | `String` | `&str` or `ArrayString<N>` |
| RS504 | `dyn Trait` | generics or enum dispatch |
| RS505 | `Arc<T>`, `Rc<T>` | cell-owned state or bounded channels |
| RS506 | `panic!()` | `Result` for recoverable, abort for unrecoverable |
| RS507 | `HashMap`, `HashSet` | `BTreeMap`, `BTreeSet`, or `BoundedMap<K,V,N>` |

## Fixing violations

When rsc reports errors, apply these patterns:

```rust
// RS501: Box::new(x) → stack value or arena
let node = arena.alloc(TreeNode { ... })?;

// RS502: Vec<T> → BoundedVec<T, N>
let items: BoundedVec<Item, 1024> = BoundedVec::new();

// RS502: vec.collect() → bounded collect
let result: BoundedVec<_, 256> = iter.try_collect()?;

// RS503: String → ArrayString or &str
let name: ArrayString<64> = ArrayString::try_from("hello")?;

// RS507: HashMap<K,V> → BTreeMap<K,V> or BoundedMap<K,V,N>
let lookup: BTreeMap<Key, Value> = BTreeMap::new();
```

## Opt-in for external crate boundaries

When wrapping an external Rust crate that uses heap types internally, opt in at module or function scope:

```rust
#[allow(rs::heap)]             // lifts RS501, RS502, RS503, RS505
#[allow(rs::dyn_dispatch)]     // lifts RS504
#[allow(rs::nondeterministic)] // lifts RS507
// RS506 (panic) has no opt-out — always enforced
```

Never apply at crate level.

## rs-lang types

Add `rs-lang` as a dependency and import the prelude:

```rust
use rs_lang::prelude::*;
```

Available types:

- `BoundedVec<T, N>` — fixed-capacity vec, `try_push()` returns Err if full
- `BoundedMap<K, V, N>` — fixed-capacity ordered map
- `ArrayString<N>` — fixed-capacity string
- `Arena<T, N>` — typed arena, compile-time max, all freed on drop
- `FixedPoint<u128, DECIMALS>` — deterministic decimal arithmetic
- `Particle` — 64-byte content-address hash (Copy, Eq, Ord)

## CLAUDE.md template for companion projects

Add this section to the `CLAUDE.md` of any project compiled with rsc:

```markdown
# rs-lang companion repo

this project is compiled with `rsc` — the rs edition compiler.
source of truth for the language spec lives in `~/git/rs/reference/`.

## companion repos

| repo | path | role |
|------|------|------|
| rs | `~/git/rs/` | compiler driver (rsc), proc-macros, core runtime types |
| this project | `~/git/<name>/` | application — must compile clean under rsc |

## building with rsc

\```nu
$env.RUSTC = ($env.HOME + "/git/rs/rsc/target/release/rsc")
$env.RUSTFLAGS = "--rs-edition"
cargo build
\```

rebuild rsc after any changes to `~/git/rs/rsc/`:

\```nu
cd ~/git/rs/rsc; cargo build --release
\```

## do not touch zones

- `~/git/rs/reference/` — canonical spec, change there first
- `~/git/rs/rsc/` — compiler driver, changes require rsc rebuild
```
