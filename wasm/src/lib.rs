pub mod parser;
mod state;

use parser::content::{children_of, encode_primitive, find_node, ContentNode, ContentValue};
use parser::reader::Reader;
use parser::schema::{fmt_type_ref, parse_schema, CerimalHeader};
use parser::types::CompressionType;
use state::{with_docs, with_docs_mut, ParsedDoc};
use wasm_bindgen::prelude::*;

// ---------------------------------------------------------------------------
// parse_save
// ---------------------------------------------------------------------------

/// Parse all CERIMAL documents in a save file.
/// dict_bytes: zstd dictionary bytes (pass empty slice for uncompressed saves).
/// Returns JSON: [{index, rootType}, ...]
#[wasm_bindgen]
pub fn parse_save(data: &[u8], dict_bytes: &[u8]) -> Result<JsValue, JsValue> {
    let mut r = Reader::from_slice(data);
    let mut docs = Vec::new();

    while r.remaining() >= 7 {
        // Peek at magic
        if &r.data[r.pos..r.pos + 7] != b"CERIMAL" {
            break;
        }

        let doc_start = r.pos;
        let header = CerimalHeader::parse(&mut r).map_err(js_err)?;
        let _schema_start = r.pos;
        let schema_bytes = r.read_bytes(header.schema_size as usize).map_err(js_err)?;
        let schema = parse_schema(&mut Reader::from_slice(&schema_bytes)).map_err(js_err)?;

        let raw_content = r.read_bytes(header.content_size as usize).map_err(js_err)?;
        let doc_end = r.pos;

        let content_bytes = decompress_content(&raw_content, header.comp_type, dict_bytes)
            .map_err(js_err)?;

        let root = parser::content::parse_document_content(&schema, &content_bytes)
            .map_err(js_err)?;

        let raw_bytes = data[doc_start..doc_end].to_vec();

        docs.push(ParsedDoc {
            raw_bytes,
            schema_bytes,
            content_bytes,
            comp_type: header.comp_type,
            schema,
            root,
        });
    }

    // Store parsed docs in thread-local state
    with_docs_mut(|store| {
        *store = docs;
    });

    // Build JSON result
    let result: Vec<serde_json::Value> = with_docs(|docs| {
        docs.iter().enumerate().map(|(i, doc)| {
            let root_type = fmt_type_ref(&doc.schema.root_type_ref, &doc.schema);
            serde_json::json!({ "index": i, "rootType": root_type })
        }).collect()
    });

    Ok(JsValue::from_str(&serde_json::to_string(&result).map_err(|e| js_err(e.to_string()))?))
}

// ---------------------------------------------------------------------------
// get_node_children
// ---------------------------------------------------------------------------

/// Get children of the node at `path` in document `doc_idx`.
/// path="" means the root node.
/// Returns JSON: [{key, path, type, isLeaf, value, childCount, guid?}, ...]
#[wasm_bindgen]
pub fn get_node_children(doc_idx: usize, path: &str) -> Result<JsValue, JsValue> {
    let result: Vec<serde_json::Value> = with_docs(|docs| {
        let doc = docs.get(doc_idx).ok_or_else(|| format!("doc {} not found", doc_idx))?;
        let node = if path.is_empty() {
            &doc.root
        } else {
            find_node(&doc.root, path).ok_or_else(|| format!("path not found: {}", path))?
        };
        let children = children_of(node, path, &doc.schema);
        let out: Vec<serde_json::Value> = children.into_iter().map(|c| {
            let mut obj = serde_json::json!({
                "key": c.key,
                "path": c.path,
                "type": c.meta.type_str,
                "isLeaf": c.meta.is_leaf,
                "value": c.meta.value,
                "childCount": c.meta.child_count,
            });
            if let Some(guid) = c.meta.guid {
                obj["guid"] = serde_json::Value::String(guid);
            }
            obj
        }).collect();
        Ok(out)
    }).map_err(|e: String| js_err(e))?;

    Ok(JsValue::from_str(&serde_json::to_string(&result).map_err(|e| js_err(e.to_string()))?))
}

// ---------------------------------------------------------------------------
// get_root_primitives
// ---------------------------------------------------------------------------

/// Get patchable primitive fields from the root composite of document `doc_idx`.
/// Returns JSON: [{path, type, value}, ...]
#[wasm_bindgen]
pub fn get_root_primitives(doc_idx: usize) -> Result<JsValue, JsValue> {
    let result: Vec<serde_json::Value> = with_docs(|docs| {
        let doc = docs.get(doc_idx).ok_or_else(|| format!("doc {} not found", doc_idx))?;
        let root = doc.root.unwrap_poly();
        let fields = match root {
            ContentNode::Concrete(n) => {
                if let ContentValue::Composite(f) = &n.value { f } else { return Ok(vec![]); }
            }
            _ => return Ok(vec![]),
        };
        let mut out = Vec::new();
        for (fname, child) in fields {
            let actual = child.unwrap_poly();
            if let ContentNode::Concrete(cn) = actual {
                if let ContentValue::Primitive(pv) = &cn.value {
                    // Only include numeric/bool primitives (no guid, string, bytes)
                    use parser::content::PrimitiveValue;
                    let is_patchable = matches!(pv, PrimitiveValue::Bool(_) | PrimitiveValue::I64(_) | PrimitiveValue::U64(_) | PrimitiveValue::F64(_) | PrimitiveValue::Fp(_) | PrimitiveValue::Lfp(_));
                    if is_patchable {
                        let type_str = fmt_type_ref(&cn.type_ref, &doc.schema);
                        out.push(serde_json::json!({
                            "path": fname,
                            "type": type_str,
                            "value": pv.to_display_string(),
                        }));
                    }
                }
            }
        }
        Ok(out)
    }).map_err(|e: String| js_err(e))?;

    Ok(JsValue::from_str(&serde_json::to_string(&result).map_err(|e| js_err(e.to_string()))?))
}

// ---------------------------------------------------------------------------
// patch_field
// ---------------------------------------------------------------------------

/// Patch a primitive field and return the full patched file bytes.
/// compress_fn: JS function (data: Uint8Array, dict: Uint8Array) -> Uint8Array
#[wasm_bindgen]
pub fn patch_field(
    doc_idx: usize,
    field_name: &str,
    value_str: &str,
    compress_fn: &js_sys::Function,
    dict_bytes: &[u8],
) -> Result<Vec<u8>, JsValue> {
    // 1. Find the node and compute the patch
    let (value_start, encoded) = with_docs(|docs| {
        let doc = docs.get(doc_idx).ok_or_else(|| format!("doc {} not found", doc_idx))?;
        let node = find_node(&doc.root, field_name)
            .ok_or_else(|| format!("field '{}' not found", field_name))?;
        let actual = node.unwrap_poly();
        if let ContentNode::Concrete(cn) = actual {
            encode_primitive(cn, value_str)
        } else {
            Err(format!("field '{}' is not a concrete node", field_name))
        }
    }).map_err(|e: String| js_err(e))?;

    // 2. Patch content_bytes, recompress, rebuild header
    let new_doc_bytes = with_docs(|docs| {
        let doc = docs.get(doc_idx).unwrap();

        // Patch content
        let mut content = doc.content_bytes.clone();
        let end = value_start + encoded.len();
        if end > content.len() {
            return Err(format!("patch overflows content_bytes: {} + {} > {}", value_start, encoded.len(), content.len()));
        }
        content[value_start..end].copy_from_slice(&encoded);

        // Recompress
        let compressed = match doc.comp_type {
            CompressionType::Zstd => {
                // Call JS compress function
                let data_js = js_sys::Uint8Array::from(content.as_slice());
                let dict_js = js_sys::Uint8Array::from(dict_bytes);
                let result = compress_fn.call2(&JsValue::null(), &data_js, &dict_js)
                    .map_err(|e| format!("compress_fn failed: {:?}", e))?;
                let arr = js_sys::Uint8Array::from(result);
                arr.to_vec()
            }
            CompressionType::None => content,
        };

        // Recompute xxHash64 over schema_bytes + compressed_content
        let checksum = xxhash_rust::xxh64::xxh64(
            &[doc.schema_bytes.as_slice(), compressed.as_slice()].concat(),
            0,
        );

        // Rebuild 28-byte header
        let comp_type_int: u32 = if doc.comp_type == CompressionType::Zstd { 1 } else { 0 };
        let mut new_header = Vec::with_capacity(28);
        new_header.extend_from_slice(b"CERIMAL");
        // version: read from original header (byte 7)
        let version = doc.raw_bytes[7];
        new_header.push(version);
        new_header.extend_from_slice(&(doc.schema_bytes.len() as u32).to_le_bytes());
        new_header.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
        new_header.extend_from_slice(&comp_type_int.to_le_bytes());
        new_header.extend_from_slice(&checksum.to_le_bytes());

        let mut result = new_header;
        result.extend_from_slice(&doc.schema_bytes);
        result.extend_from_slice(&compressed);
        Ok(result)
    }).map_err(|e: String| js_err(e))?;

    // 3. Concatenate all docs: patched replaces original
    let full_bytes: Vec<u8> = with_docs(|docs| {
        let mut out = Vec::new();
        for (i, doc) in docs.iter().enumerate() {
            if i == doc_idx {
                out.extend_from_slice(&new_doc_bytes);
            } else {
                out.extend_from_slice(&doc.raw_bytes);
            }
        }
        out
    });

    Ok(full_bytes)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn js_err<E: std::fmt::Display>(e: E) -> JsValue {
    JsValue::from_str(&e.to_string())
}

fn decompress_content(
    data: &[u8],
    comp_type: CompressionType,
    dict_bytes: &[u8],
) -> Result<Vec<u8>, String> {
    match comp_type {
        CompressionType::None => Ok(data.to_vec()),
        CompressionType::Zstd => {
            use std::io::Read;
            let mut out = Vec::new();
            // with_dictionary works for both empty and non-empty dicts
            zstd::stream::read::Decoder::with_dictionary(data, dict_bytes)
                .map_err(|e| format!("zstd init: {}", e))?
                .read_to_end(&mut out)
                .map_err(|e| format!("zstd decompress: {}", e))?;
            Ok(out)
        }
    }
}
