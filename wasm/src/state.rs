use std::cell::RefCell;
use crate::parser::content::ContentNode;
use crate::parser::schema::CerimalSchema;
use crate::parser::types::CompressionType;

pub struct ParsedDoc {
    pub raw_bytes: Vec<u8>,
    pub schema_bytes: Vec<u8>,
    pub content_bytes: Vec<u8>,  // decompressed content
    pub comp_type: CompressionType,
    pub schema: CerimalSchema,
    pub root: ContentNode,
}

thread_local! {
    static DOCS: RefCell<Vec<ParsedDoc>> = RefCell::new(Vec::new());
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
