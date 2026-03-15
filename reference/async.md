---
tags: cyber, rs, reference
---

# Bounded Async

## Problem

Rust's `async fn` creates futures with no deadline. A forgotten `.await` on a network read can block a task forever. In OS kernels and blockchain nodes, this is not a bug — it is a liveness failure that can cost real money (slashing) or crash a system.

Existing workarounds (`tokio::time::timeout()`) are opt-in and forgettable. They are library-level, not language-level.

## Solution

Rs extends `async` with an optional deadline parameter.

## Syntax

```rust
// Standard Rust async — still valid, still works
async fn standard_function() -> Result<()> {
    // ...
}

// Rs bounded async — deadline is part of the function signature
async(100ms) fn read_block(lba: u64) -> Result<Block> {
    let data = device.read(lba).await;  // .await inherits 100ms deadline
    Ok(Block::from(data))
}

// Duration expressions allowed
async(Duration::from_secs(5)) fn sync_state() -> Result<()> {
    // ...
}

// Constant expressions allowed
const CONSENSUS_TIMEOUT: Duration = Duration::from_millis(500);
async(CONSENSUS_TIMEOUT) fn propose_block() -> Result<Block> {
    // ...
}
```

## Semantics

When `async(D) fn foo() -> T` is called:

1. An internal timer starts with duration `D`
2. Every `.await` inside the function checks the remaining time
3. If the timer expires before the function returns, the future resolves to `Err(Rs::Timeout)`
4. The return type is transparently wrapped: `async(D) fn foo() -> T` actually returns `Result<T, Rs::Timeout>` at the future level

Nested calls:

```rust
async(200ms) fn outer() -> Result<()> {
    // inner gets at most the REMAINING time of outer, not its own 100ms
    // if outer has 50ms left, inner's effective deadline is 50ms
    let result = inner().await;
    Ok(())
}

async(100ms) fn inner() -> Result<Data> {
    // ...
}
```

The effective deadline is `min(own_deadline, caller_remaining)`. Deadlines propagate inward, never expand.

## Rs Edition Enforcement

In `edition = "rs"`:

```rust
// ERROR in rs edition: async fn without deadline
async fn unbounded() -> Result<()> {
    //~^ error[RS101]: async functions must have a deadline in rs edition
    //~| help: add a deadline: async(100ms) fn unbounded()
}

// OK: explicit opt-out for rare cases (must justify)
#[allow(rs::unbounded_async)]
async fn special_case() -> Result<()> {
    // ...
}
```

In standard Rust editions, `async(duration)` is available but not required.

## Compiler Implementation

The parser recognizes `async ( <expr> )` as a bounded async marker. The desugaring:

```rust
// Source:
async(100ms) fn foo(x: u32) -> Result<Bar> {
    let a = something().await;
    Ok(a.into())
}

// Desugared to (approximately):
fn foo(x: u32) -> impl Future<Output = Result<Result<Bar>, rs::Timeout>> {
    rs::runtime::with_deadline(Duration::from_millis(100), async move {
        let a = something().await;
        Ok(a.into())
    })
}
```

Parser changes: ~200 lines. Desugaring: ~300 lines. Diagnostic messages: ~100 lines.
