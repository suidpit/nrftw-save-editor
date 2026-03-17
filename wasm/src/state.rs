use crate::document::ParsedDoc;
use std::cell::RefCell;

// Thread-local storage is used here because WASM is single-threaded and
// wasm-bindgen doesn't support passing complex owned state across FFI calls.
// The parsed documents are stored once by `parse_save` and then accessed by
// index from subsequent JS calls (get_node_children, patch_field, etc.).
thread_local! {
    static DOCS: RefCell<Vec<ParsedDoc>> = const { RefCell::new(Vec::new()) };
}

pub fn with_docs<F, R>(f: F) -> R
where
    F: FnOnce(&Vec<ParsedDoc>) -> R,
{
    DOCS.with(|docs| f(&docs.borrow()))
}

pub fn with_docs_mut<F, R>(f: F) -> R
where
    F: FnOnce(&mut Vec<ParsedDoc>) -> R,
{
    DOCS.with(|docs| f(&mut docs.borrow_mut()))
}
