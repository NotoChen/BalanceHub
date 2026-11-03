//! Read-scoped reuse only. Never retains filesystem authority or write guards.
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{cell::RefCell, collections::HashMap, marker::PhantomData, path::PathBuf, rc::Rc};

const MAX_PATHS: usize = 16_384;
const MAX_PARSED_BYTES: usize = 8 * 1024 * 1024;

#[derive(Default)]
struct State {
    depth: usize,
    paths: HashMap<(PathBuf, bool), [u8; 32]>,
    documents: HashMap<([u8; 32], u8), Value>,
    parsed_bytes: usize,
}

thread_local! {
    static READ: RefCell<State> = RefCell::new(State::default());
}

/// Must stay within one synchronous read. Each outer read drops its cache;
/// before/after freshness checks deliberately open separate scopes.
pub(crate) struct ReadScope {
    previous: Option<State>,
    _not_send: PhantomData<Rc<()>>,
}

impl ReadScope {
    pub(crate) fn enter() -> Self {
        READ.with(|state| state.borrow_mut().depth += 1);
        Self {
            previous: None,
            _not_send: PhantomData,
        }
    }

    /// Fingerprints delimit fresh observations even when called from another
    /// read scope. Nested scan caching must not hide a before/after change.
    pub(crate) fn fresh() -> Self {
        let previous = READ.with(|state| {
            std::mem::replace(
                &mut *state.borrow_mut(),
                State {
                    depth: 1,
                    ..State::default()
                },
            )
        });
        Self {
            previous: Some(previous),
            _not_send: PhantomData,
        }
    }
}

impl Drop for ReadScope {
    fn drop(&mut self) {
        READ.with(|state| {
            let mut state = state.borrow_mut();
            if let Some(previous) = self.previous.take() {
                *state = previous;
                return;
            }
            state.depth -= 1;
            if state.depth == 0 {
                *state = State::default();
            }
        });
    }
}

pub(super) fn path(
    path: &std::path::Path,
    contents: bool,
    read: impl FnOnce() -> [u8; 32],
) -> [u8; 32] {
    let key = (path.to_path_buf(), contents);
    if let Some(value) = READ.with(|state| state.borrow().paths.get(&key).copied()) {
        return value;
    }
    let value = read();
    READ.with(|state| {
        let mut state = state.borrow_mut();
        if state.depth > 0 && state.paths.len() < MAX_PATHS {
            state.paths.insert(key, value);
        }
    });
    value
}

/// The key includes the complete bytes, so even same-size, same-mtime rewrites
/// cannot reuse a stale parse. Callers still read through their verified handle.
pub(crate) fn parsed(
    bytes: &[u8],
    format: u8,
    parse: impl FnOnce() -> Option<Value>,
) -> Option<Value> {
    if !READ.with(|state| state.borrow().depth > 0) || bytes.len() > MAX_PARSED_BYTES {
        return parse();
    }
    let key = (Sha256::digest(bytes).into(), format);
    if let Some(value) = READ.with(|state| state.borrow().documents.get(&key).cloned()) {
        return Some(value);
    }
    let value = parse()?;
    READ.with(|state| {
        let mut state = state.borrow_mut();
        // Bound both the source and parsed representation, including tiny files.
        let cost = bytes.len().saturating_mul(4).saturating_add(256);
        if state.parsed_bytes.saturating_add(cost) <= MAX_PARSED_BYTES {
            state.parsed_bytes += cost;
            state.documents.insert(key, value.clone());
        }
    });
    Some(value)
}
