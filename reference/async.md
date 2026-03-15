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

## Time Source

The time source backing `with_deadline` is provided by the runtime, not by the compiler. For deterministic systems (consensus nodes), the runtime must use logical time (step-based or block-height-based) so that deadline expiration is identical across all nodes. Wall-clock time is acceptable only in non-deterministic contexts. Rs enforces the *presence* of a deadline; the runtime determines the *clock* that measures it.

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

## Implementation

Code examples below use the `rs::` logical namespace. In Rust code, import as `rs_lang::` (see [stdlib.md](stdlib.md)).

Inside `cell!` macro: the macro parses `async(dur) fn` from its own token stream and generates the timeout wrapping. The deadline expression must be a const expression of type `Duration`.

Outside cells: the `#[bounded_async(dur)]` attribute macro provides the same functionality with standard Rust syntax:

```rust
// Inside cell! — custom syntax, parsed by macro:
pub async(Duration::from_millis(100)) fn fetch(&self) -> Result<Item, AppError> { ... }

// Outside cell! — standard attribute syntax:
#[bounded_async(Duration::from_millis(100))]
async fn fetch(id: u64) -> Result<Item, AppError> { ... }
```

Both desugar to the same code:

```rust
// Desugared (approximately):
fn fetch(id: u64) -> impl Future<Output = Result<Item, AppError>> {
    rs::runtime::with_deadline(Duration::from_millis(100), async move {
        let a = something().await?;
        Ok(a.into())
    })
    // on timeout: returns Err(AppError::from(rs::Timeout))
}
```

No rustc parser modification needed. The `async(dur)` syntax only exists inside `cell!` token streams.

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

Implementation: the `cell!` macro handles `async(dur)` syntax internally (~included in cell macro line count). Outside cells, `#[bounded_async(dur)]` attribute macro provides the same functionality (~200 lines in rs-lang-macros). No rustc parser modification needed. Diagnostic messages: ~100 lines.

## Error Reference

See [errors/async.md](errors/async.md) for detailed description of RS101.
