---
tags: cyber, rs, reference
---

# Cell Declarations

## Compiler vs Runtime Boundary

Rs is a compiler. It defines the *shape* of a cell and enforces structural correctness at compile time. The runtime consumes cell declarations and provides lifecycle management.

**Rs enforces (compile time):**
- State and step_state structs are well-formed
- Migration implementations type-check against previous version
- Step state resets are generated correctly
- Public interface is introspectable via `CellMetadata`
- Bounded async deadlines are present on async methods

**Rs declares (consumed by runtime):**
- `budget` — resource limit per cell; the runtime enforces it
- `heartbeat` — liveness interval; the runtime monitors it
- `health_check()` — the runtime calls it; the cell implements it
- Channel connections — the runtime wires them at initialization
- Hot-swap protocol — the runtime drives step boundaries and triggers migration

Rs validates that these declarations are syntactically and type-correct. The runtime decides what to do with them.

## Problem

Operating system modules need: a private state, a public interface, a resource budget, health monitoring, hot-swap capability, and state migration between versions. Rust has none of these as a first-class concept. Crates provide modularity but not lifecycle management.

## Solution

The `cell!` macro declares a self-contained, hot-swappable OS module.

## Syntax

```rust
cell! {
    name: Consensus,
    version: 3,
    budget: Duration::from_millis(500),
    heartbeat: Duration::from_secs(1),

    state {
        validators: BTreeMap<Address, StakeAmount>,
        current_round: u64,
        votes: BoundedVec<Vote, MAX_VALIDATORS>,
    }

    // Step-scoped state (auto-reset each step)
    step_state {
        round_votes: BoundedVec<Vote, MAX_VALIDATORS>,
        proposed_block: Option<Block>,
    }

    // Public interface — other cells can call these
    pub fn propose_block(&self, txs: &[Transaction]) -> Result<Block> {
        // ...
    }

    pub async(Duration::from_millis(200)) fn vote(&mut self, block: &Block) -> Result<Vote> {
        // ...
    }

    pub fn validator_set(&self) -> &BTreeMap<Address, StakeAmount> {
        &self.state.validators
    }

    // Private functions — internal to cell
    fn verify_proposer(&self, proposer: Address) -> bool {
        self.state.validators.contains_key(&proposer)
    }

    // State migration from previous version
    migrate from v2 {
        validators: old.validators,
        current_round: old.current_round,
        votes: BoundedVec::new(), // new field in v3
    }
}
```

## Generated Code

The `cell!` macro generates:

```rust
// 1. State struct
pub struct ConsensusState {
    validators: BTreeMap<Address, StakeAmount>,
    current_round: u64,
    votes: BoundedVec<Vote, MAX_VALIDATORS>,
}

// 2. Step state struct (with auto-reset)
#[step]
pub struct ConsensusStepState {
    round_votes: BoundedVec<Vote, MAX_VALIDATORS>,
    proposed_block: Option<Block>,
}

// 3. Cell struct wrapping both
pub struct Consensus {
    state: ConsensusState,
    step_state: ConsensusStepState,
}

// 4. Cell trait implementation
impl Cell for Consensus {
    const NAME: &'static str = "Consensus";
    const VERSION: u32 = 3;
    const BUDGET: Duration = Duration::from_millis(500);
    const HEARTBEAT: Duration = Duration::from_secs(1);

    fn current_step(&self) -> u64;
    fn health_check(&self) -> HealthStatus { /* ... */ }
    fn reset_step_state(&mut self) { self.step_state.reset(); }
}

// 5. Migration from v2
impl MigrateFrom<ConsensusStateV2> for ConsensusState {
    fn migrate(old: ConsensusStateV2) -> Self {
        Self {
            validators: old.validators,
            current_round: old.current_round,
            votes: BoundedVec::new(),
        }
    }
}

// 6. Public interface methods (impl block)
impl Consensus {
    pub fn propose_block(&self, txs: &[Transaction]) -> Result<Block> { /* ... */ }
    pub fn vote(&mut self, block: &Block) -> impl Future<Output = Result<Vote>> {
        rs::runtime::with_deadline(Duration::from_millis(200), async move { /* ... */ })
    }
    pub fn validator_set(&self) -> &BTreeMap<Address, StakeAmount> { /* ... */ }
    fn verify_proposer(&self, proposer: Address) -> bool { /* ... */ }
}

// 7. Metadata for introspection
impl CellMetadata for Consensus {
    fn interface() -> &'static [FunctionSignature] {
        &[
            FunctionSignature { name: "propose_block", args: &["&[Transaction]"], ret: "Result<Block>" },
            FunctionSignature { name: "vote", args: &["&Block"], ret: "Result<Vote>", deadline: Some(Duration::from_millis(200)) },
            FunctionSignature { name: "validator_set", args: &[], ret: "&BTreeMap<Address, StakeAmount>" },
        ]
    }
}
```

## Migration State Schema

The `migrate from vN` block generates `MigrateFrom<XxxStateVN>` impl. The previous version's state struct must be in scope at compile time. Convention: keep old state definitions in versioned modules alongside the cell.

```rust
// Previous version state — kept for migration
mod v2 {
    pub struct ConsensusState {
        pub validators: BTreeMap<Address, StakeAmount>,
        pub current_round: u64,
    }
}

// Current cell uses v2::ConsensusState as the migration source
cell! {
    name: Consensus,
    version: 3,
    // ...
    migrate from v2 {
        validators: old.validators,
        current_round: old.current_round,
        votes: BoundedVec::new(),
    }
}
```

The `old` binding has the type of the previous state struct (`v2::ConsensusState`). The macro resolves `vN` to `{CellName}StateVN` (e.g. `ConsensusStateV2`) by convention. If a different type name is needed, use the full path: `migrate from my_module::OldState { ... }`.

## Hot-Swap Protocol

cyb os is a living system. Living systems replace their components continuously without stopping — biological cells divide, differentiate, and die while the organism keeps running. cyb os cells follow the same principle: the system never reboots, modules update in place, and state migrates forward version by version.

The hot-swap protocol is a mechanical process. The spec defines *how* cells swap, not *who* or *what* triggers the swap. The trigger is external to the protocol — it could be a local operator, a governance decision, an automated upgrade pipeline, or the cell itself detecting that a new version is available. The protocol is the same regardless.

```
 Step N           Step N+1         Step N+2
 ┌──────┐        ┌──────────┐     ┌──────┐
 │Cell  │  drain │Cell v2    │     │Cell  │
 │v1    │───────>│loading +  │────>│v2    │
 │active│        │migration  │     │active│
 └──────┘        └──────────┘     └──────┘
                  state transfer
                  via MigrateFrom
```

1. New cell version is loaded (trigger is external to the protocol)
2. Current step completes normally
3. At step boundary: old cell's state is serialized via `CanonicalSerialize`
4. Serialized bytes are deserialized into the previous version's state struct (`XxxStateVN`)
5. `MigrateFrom::migrate()` transforms the old state struct to the new version
6. New cell is initialized with migrated state
7. New cell starts processing next step
8. Old cell binary is unloaded

Total downtime: zero. Migration happens in the gap between steps. The system never stops.

## Error Types

The `cell!` macro generates a cell-specific error enum from the error variants used in the cell's methods. Error variants are referenced as `Error::VariantName` in the cell body. The macro collects all referenced variants and generates:

```rust
#[derive(Debug)]
pub enum ConsensusError {
    GraphFull,
    AgentLimitReached,
    StepFull,
    Overflow,
    Timeout,  // auto-added if any method uses bounded async
}

impl From<rs::Timeout> for ConsensusError {
    fn from(_: rs::Timeout) -> Self { ConsensusError::Timeout }
}
```

Inside the cell body, `Error::GraphFull` resolves to `ConsensusError::GraphFull`. The `Result` type inside cell methods is `Result<T, ConsensusError>`.

## Cell-to-Cell Communication

Cells communicate through two mechanisms:

1. **Bounded channels** — the `cell!` macro can declare typed input/output channels:

```rust
cell! {
    name: Router,
    // ...
    input: BoundedChannel<Transaction, 1000>,
    output: BoundedChannel<Block, 10>,
}
```

Channels are wait-free (try_send/try_recv). The cell runtime connects channels between cells at initialization.

2. **Read-only state references** — a cell can declare a dependency on another cell's public interface. The runtime provides a read-only reference at initialization. The dependent cell calls methods on this reference but cannot mutate the other cell's state.

Implementation: proc-macro, ~2000 lines.
