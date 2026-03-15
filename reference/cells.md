---
tags: cyber, rs, reference
---

# Cell Declarations

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

    // Epoch-scoped state (auto-reset each block)
    epoch_state {
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

// 2. Epoch state struct (with auto-reset)
#[epoch]
pub struct ConsensusEpochState {
    round_votes: BoundedVec<Vote, MAX_VALIDATORS>,
    proposed_block: Option<Block>,
}

// 3. Cell struct wrapping both
pub struct Consensus {
    state: ConsensusState,
    epoch_state: ConsensusEpochState,
}

// 4. Cell trait implementation
impl Cell for Consensus {
    const NAME: &'static str = "Consensus";
    const VERSION: u32 = 3;
    const BUDGET: Duration = Duration::from_millis(500);
    const HEARTBEAT: Duration = Duration::from_secs(1);

    fn current_epoch(&self) -> u64;
    fn health_check(&self) -> HealthStatus { /* ... */ }
    fn reset_epoch_state(&mut self) { self.epoch_state.reset(); }
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
    pub async(Duration::from_millis(200)) fn vote(&mut self, block: &Block) -> Result<Vote> { /* ... */ }
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
 Epoch N          Epoch N+1        Epoch N+2
 ┌──────┐        ┌──────────┐     ┌──────┐
 │Cell  │  drain │Cell v2    │     │Cell  │
 │v1    │───────>│loading +  │────>│v2    │
 │active│        │migration  │     │active│
 └──────┘        └──────────┘     └──────┘
                  state transfer
                  via MigrateFrom
```

1. New cell version is loaded (trigger is external to the protocol)
2. Current epoch completes normally
3. At epoch boundary: old cell's state is serialized via `CanonicalSerialize`
4. `MigrateFrom::migrate()` transforms state to new version
5. New cell is initialized with migrated state
6. New cell starts processing next epoch
7. Old cell binary is unloaded

Total downtime: zero. Migration happens in the gap between epochs. The system never stops.

## Error Types

The `cell!` macro generates a cell-specific error enum from the error variants used in the cell's methods. Error variants are referenced as `Error::VariantName` in the cell body. The macro collects all referenced variants and generates:

```rust
#[derive(Debug)]
pub enum ConsensusError {
    GraphFull,
    AgentLimitReached,
    EpochFull,
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
