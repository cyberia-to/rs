use core::{future::Future, task::{Context, Poll, Waker}, time::Duration};
use rs_lang::{module, MigrateFrom, Module, ModuleMetadata};

pub struct CounterStateV1 { pub count: u64 }
module! {
    name: Counter,
    version: 2,
    budget: Duration::from_millis(20),
    heartbeat: Duration::from_secs(1),
    state { count: u64, upgrades: u32 }
    step_state { calls: u32 }
    migrate from v1 { count: old.count, upgrades: 1 }
    pub async(Duration::from_millis(5)) fn increment(&mut self) -> Result<u64> {
        self.state.count += 1;
        self.step_state.calls += 1;
        Ok(self.state.count)
    }
    pub async(Duration::from_millis(2)) fn static_value() -> Result<u64> { Ok(42) }
}

#[test]
fn state_upgrade_keeps_value_step_reset_and_deadline_metadata() {
    let mut module = Counter::new();
    module.state = CounterState::migrate(CounterStateV1 { count: 41 });
    assert_eq!(module.state.upgrades, 1);
    let mut cx = Context::from_waker(Waker::noop());
    {
        let mut future = core::pin::pin!(module.increment());
        assert!(matches!(future.as_mut().poll(&mut cx), Poll::Ready(Ok(42))));
    }
    assert_eq!(module.step_state.calls, 1);
    module.reset_step_state();
    assert_eq!(module.step_state.calls, 0);
    assert_eq!(module.state.count, 42);
    assert_eq!(Counter::VERSION, 2);
    let interface = Counter::interface();
    assert_eq!(interface[0].deadline, Some(Duration::from_millis(5)));
    assert_eq!(interface[1].deadline, Some(Duration::from_millis(2)));
    let mut future = core::pin::pin!(Counter::static_value());
    assert!(matches!(future.as_mut().poll(&mut cx), Poll::Ready(Ok(42))));
}

#[allow(deprecated)]
mod old_source {
    use super::*;
    rs_lang::cell! {
        name: OldSpelling,
        version: 1,
        budget: Duration::from_millis(20),
        heartbeat: Duration::from_secs(1),
        state { count: u64 }
    }
    fn canonical<T: Module + ModuleMetadata>() {}
    fn compatible<T: rs_lang::Cell + rs_lang::CellMetadata>() { canonical::<T>(); }
    #[test]
    fn old_api_delegates_to_the_same_module_traits() { compatible::<OldSpelling>(); }
}
