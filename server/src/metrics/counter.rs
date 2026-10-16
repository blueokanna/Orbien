use std::sync::atomic::{AtomicI64, Ordering};
#[derive(Debug, Default)]
pub struct Counter {
    value: AtomicI64,
}
impl Counter {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn inc(&self, delta: i64) {
        self.value.fetch_add(delta, Ordering::Relaxed);
    }
    pub fn dec(&self, delta: i64) {
        self.value.fetch_sub(delta, Ordering::Relaxed);
    }
    pub fn count(&self) -> i64 {
        self.value.load(Ordering::Relaxed)
    }
}
