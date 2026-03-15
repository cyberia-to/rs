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
    budget: 500ms,
    heartbeat: 1s,

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

    pub async(200ms) fn vote(&mut self, block: &Block) -> Result<Vote> {
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
    pub async(200ms) fn vote(&mut self, block: &Block) -> Result<Vote> { /* ... */ }
    pub fn validator_set(&self) -> &BTreeMap<Address, StakeAmount> { /* ... */ }
    fn verify_proposer(&self, proposer: Address) -> bool { /* ... */ }
}

// 7. Metadata for introspection
impl CellMetadata for Consensus {
    fn interface() -> &'static [FunctionSignature] {
        &[
            FunctionSignature { name: "propose_block", args: &["&[Transaction]"], ret: "Result<Block>" },
            FunctionSignature { name: "vote", args: &["&Block"], ret: "Result<Vote>", deadline: Some(200) },
            FunctionSignature { name: "validator_set", args: &[], ret: "&BTreeMap<Address, StakeAmount>" },
        ]
    }
}
```

## Hot-Swap Protocol

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

1. Governance approves new cell binary
2. Current epoch completes normally
3. At epoch boundary: old cell's state is serialized
4. `MigrateFrom::migrate()` transforms state to new version
5. New cell is initialized with migrated state
6. New cell starts processing next epoch
7. Old cell binary is unloaded

Total downtime: zero. Migration happens in the gap between epochs.

Implementation: proc-macro, ~2000 lines.
