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
async(Duration::from_millis(100)) fn read_block(lba: u64) -> Result<Block> {
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
3. If the timer expires before the function returns, the future resolves to a timeout error
4. The function must return `Result<T, E>` where `E: From<rs::Timeout>`. On timeout, the future resolves to `Err(E::from(rs::Timeout))`

Nested calls:

```rust
async(Duration::from_millis(200)) fn outer() -> Result<()> {
    // inner gets at most the REMAINING time of outer, not its own 100ms
    // if outer has 50ms left, inner's effective deadline is 50ms
    let result = inner().await;
    Ok(())
}

async(Duration::from_millis(100)) fn inner() -> Result<Data> {
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
    //~| help: add a deadline: async(Duration::from_millis(100)) fn unbounded()
}

// OK: explicit opt-out for rare cases (must justify)
#[allow(rs::unbounded_async)]
async fn special_case() -> Result<()> {
    // ...
}
```

In standard Rust editions, `async(duration)` is available but not required.

## Compiler Implementation

The parser recognizes `async ( <expr> )` as a bounded async marker. The deadline expression must be a const expression of type `Duration`.

The desugaring:

```rust
// Source:
async(Duration::from_millis(100)) fn foo(x: u32) -> Result<Bar, AppError> {
    let a = something().await?;
    Ok(a.into())
}

// Desugared to (approximately):
fn foo(x: u32) -> impl Future<Output = Result<Bar, AppError>> {
    rs::runtime::with_deadline(Duration::from_millis(100), async move {
        let a = something().await?;
        Ok(a.into())
    })
    // on timeout: returns Err(AppError::from(rs::Timeout))
}
```

The timeout marker type:

```rust
/// Unit struct returned when a bounded async function exceeds its deadline.
pub struct rs::Timeout;
```

The bounded async function's error type must implement `From<rs::Timeout>`. For functions where timeout is the only error, use `rs::Timeout` directly as the error type:

```rust
async(Duration::from_millis(50)) fn simple_read() -> Result<Data, rs::Timeout> {
    // ...
}
```

For functions with application-specific errors, include a timeout variant:

```rust
enum AppError {
    NotFound,
    InvalidData,
    Timeout,
}

impl From<rs::Timeout> for AppError {
    fn from(_: rs::Timeout) -> Self { AppError::Timeout }
}

async(Duration::from_millis(100)) fn fetch(id: u64) -> Result<Item, AppError> {
    // on timeout: Err(AppError::Timeout)
    // on app error: Err(AppError::NotFound), etc.
}
```

Parser changes: ~200 lines. Desugaring: ~300 lines. Diagnostic messages: ~100 lines.
