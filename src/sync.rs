#[cfg(loom)]
pub(crate) use loom::sync::{Arc, Mutex};

#[cfg(not(loom))]
pub(crate) use std::sync::{Arc, Mutex};
