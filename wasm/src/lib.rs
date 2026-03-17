pub mod document;
pub mod dumper;
pub mod mutations;
pub mod parser;
mod state;

use parser::content::{children_of, find_node, ContentNode, ContentValue};
use parser::schema::fmt_type_ref;
use serde::{Deserialize, Serialize};
use state::{with_docs, with_docs_mut};
use wasm_bindgen::prelude::*;

// ---------------------------------------------------------------------------
// parse_save
// ---------------------------------------------------------------------------

/// Parse all CERIMAL documents in a save file.
/// dict_bytes: zstd dictionary bytes (pass empty slice for uncompressed saves).
/// Returns JSON: [{index, rootType}, ...]
#[wasm_bindgen]
pub fn parse_save(data: &[u8], dict_bytes: &[u8]) -> Result<JsValue, JsValue> {
    let docs = document::parse_all_docs(data, dict_bytes).map_err(js_err)?;

    with_docs_mut(|store| {
        *store = docs;
    });

    let result: Vec<serde_json::Value> = with_docs(|docs| {
        docs.iter()
            .enumerate()
            .map(|(i, doc)| {
                let root_type = fmt_type_ref(&doc.schema.root_type_ref, &doc.schema);
                serde_json::json!({ "index": i, "rootType": root_type })
            })
            .collect()
    });

    Ok(JsValue::from_str(
        &serde_json::to_string(&result).map_err(|e| js_err(e.to_string()))?,
    ))
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
        let doc = docs
            .get(doc_idx)
            .ok_or_else(|| format!("doc {} not found", doc_idx))?;
        let node = if path.is_empty() {
            &doc.root
        } else {
            find_node(&doc.root, path).ok_or_else(|| format!("path not found: {}", path))?
        };
        let children = children_of(node, path, &doc.schema);
        let out: Vec<serde_json::Value> = children
            .into_iter()
            .map(|c| {
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
            })
            .collect();
        Ok(out)
    })
    .map_err(|e: String| js_err(e))?;

    Ok(JsValue::from_str(
        &serde_json::to_string(&result).map_err(|e| js_err(e.to_string()))?,
    ))
}

// ---------------------------------------------------------------------------
// get_root_primitives
// ---------------------------------------------------------------------------

/// Get patchable primitive fields from the root composite of document `doc_idx`.
/// Returns JSON: [{path, type, value}, ...]
#[wasm_bindgen]
pub fn get_root_primitives(doc_idx: usize) -> Result<JsValue, JsValue> {
    let result: Vec<serde_json::Value> = with_docs(|docs| {
        let doc = docs
            .get(doc_idx)
            .ok_or_else(|| format!("doc {} not found", doc_idx))?;
        let root = doc.root.unwrap_poly();
        let fields = match root {
            ContentNode::Concrete(n) => {
                if let ContentValue::Composite(f) = &n.value {
                    f
                } else {
                    return Ok(vec![]);
                }
            }
            _ => return Ok(vec![]),
        };
        let mut out = Vec::new();
        for (fname, child) in fields {
            let actual = child.unwrap_poly();
            if let ContentNode::Concrete(cn) = actual {
                if let ContentValue::Primitive(pv) = &cn.value {
                    use parser::content::PrimitiveValue;
                    let is_patchable = matches!(
                        pv,
                        PrimitiveValue::Bool(_)
                            | PrimitiveValue::I64(_)
                            | PrimitiveValue::U64(_)
                            | PrimitiveValue::F64(_)
                            | PrimitiveValue::Fp(_)
                            | PrimitiveValue::Lfp(_)
                    );
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
    })
    .map_err(|e: String| js_err(e))?;

    Ok(JsValue::from_str(
        &serde_json::to_string(&result).map_err(|e| js_err(e.to_string()))?,
    ))
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InventorySnapshotItem {
    pub index: usize,
    pub item_path: String,
    pub asset_guid: String,
    pub level: i64,
    pub rarity_num: i64,
    pub durability: i64,
    pub stack_count: i64,
    pub slot_num: i64,
    pub rune_guids: Vec<String>,
    pub enchantment_guids: Vec<String>,
    pub trait_guid: String,
}

#[wasm_bindgen]
pub fn get_inventory_snapshot(doc_idx: usize) -> Result<JsValue, JsValue> {
    let result = with_docs(|docs| {
        let doc = docs
            .get(doc_idx)
            .ok_or_else(|| format!("doc {} not found", doc_idx))?;
        inventory_snapshot(doc)
    })
    .map_err(js_err)?;

    Ok(JsValue::from_str(
        &serde_json::to_string(&result).map_err(|e| js_err(e.to_string()))?,
    ))
}

// ---------------------------------------------------------------------------
// patch_field
// ---------------------------------------------------------------------------

/// Patch a primitive field and return the full patched file bytes.
#[wasm_bindgen]
pub fn patch_field(
    doc_idx: usize,
    field_name: &str,
    value_str: &str,
    dict_bytes: &[u8],
) -> Result<Vec<u8>, JsValue> {
    let new_doc_bytes = with_docs(|docs| {
        let doc = docs
            .get(doc_idx)
            .ok_or_else(|| format!("doc {} not found", doc_idx))?;
        let compress =
            |content: &[u8]| dumper::compress_content(doc.comp_type, content, dict_bytes);
        document::patch_doc(doc, field_name, value_str, compress)
    })
    .map_err(js_err)?;

    // Concatenate all docs: patched replaces original
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

#[wasm_bindgen]
pub fn apply_changes(changes_json: &str, dict_bytes: &[u8]) -> Result<Vec<u8>, JsValue> {
    let req: mutations::ApplyChangesRequest = serde_json::from_str(changes_json).map_err(js_err)?;

    with_docs_mut(|docs| mutations::apply_changes(docs, req, dict_bytes)).map_err(js_err)
}

#[wasm_bindgen]
pub fn force_dump_all(dict_bytes: &[u8]) -> Result<Vec<u8>, JsValue> {
    with_docs(|docs| dumper::try_force_dump_all_docs(docs, dict_bytes)).map_err(js_err)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn js_err<E: std::fmt::Display>(e: E) -> JsValue {
    JsValue::from_str(&e.to_string())
}

use mutations::UNEQUIPPED_SLOT;

pub fn inventory_snapshot(doc: &document::ParsedDoc) -> Result<Vec<InventorySnapshotItem>, String> {
    let inventory =
        find_node(&doc.root, "Inventory").ok_or_else(|| "Inventory not found".to_string())?;
    let ContentNode::Concrete(node) = inventory.unwrap_poly() else {
        return Err("Inventory node is not concrete".to_string());
    };
    let ContentValue::Array { elements, .. } = &node.value else {
        return Err("Inventory node is not an array".to_string());
    };

    Ok(elements
        .iter()
        .enumerate()
        .map(|(index, entry)| InventorySnapshotItem {
            index,
            item_path: format!("Inventory[{index}]"),
            asset_guid: node_guid(entry, "Item.ItemData.Id", &doc.schema).unwrap_or_default(),
            level: node_i64(entry, "Item.Level", &doc.schema).unwrap_or(0),
            rarity_num: node_i64(entry, "Item.Rarity", &doc.schema).unwrap_or(0),
            durability: node_i64(entry, "Item.Durability.Durability", &doc.schema).unwrap_or(0),
            stack_count: node_i64(entry, "Item.Stack.Count", &doc.schema).unwrap_or(1),
            slot_num: match node_i64(entry, "Slot", &doc.schema) {
                Some(UNEQUIPPED_SLOT) => -1,
                Some(value) => value,
                None => -1,
            },
            rune_guids: rune_guids(entry, &doc.schema),
            enchantment_guids: enchantment_guids(entry, &doc.schema),
            trait_guid: {
                let guid = node_guid(entry, "Item.Enchantment.value.Trait.Asset.Id", &doc.schema)
                    .unwrap_or_default();
                if guid == "0" { String::new() } else { guid }
            },
        })
        .collect())
}

fn node_i64(
    node: &ContentNode,
    path: &str,
    schema: &crate::parser::schema::CerimalSchema,
) -> Option<i64> {
    let value = find_node(node, path)?;
    let meta = parser::content::node_meta(value, schema);
    meta.value?.parse().ok()
}

fn node_guid(
    node: &ContentNode,
    path: &str,
    schema: &crate::parser::schema::CerimalSchema,
) -> Option<String> {
    let value = find_node(node, path)?;
    parser::content::node_meta(value, schema).guid
}

fn enchantment_guids(
    entry: &ContentNode,
    schema: &crate::parser::schema::CerimalSchema,
) -> Vec<String> {
    let Some(list) = find_node(entry, "Item.Enchantment.value.Enchantment") else {
        return Vec::new();
    };
    let ContentNode::Concrete(node) = list.unwrap_poly() else {
        return Vec::new();
    };
    let ContentValue::Array { elements, .. } = &node.value else {
        return Vec::new();
    };

    elements
        .iter()
        .filter_map(|enchantment| node_guid(enchantment, "Asset.Id", schema))
        .collect()
}

fn rune_guids(
    entry: &ContentNode,
    schema: &crate::parser::schema::CerimalSchema,
) -> Vec<String> {
    let Some(list) = find_node(entry, "Item.Weapon.value.RuneSlots") else {
        return Vec::new();
    };
    let ContentNode::Concrete(node) = list.unwrap_poly() else {
        return Vec::new();
    };
    let ContentValue::Array { elements, .. } = &node.value else {
        return Vec::new();
    };

    elements
        .iter()
        .filter_map(|slot| node_guid(slot, "RuneDataId.Id", schema))
        .filter(|guid| guid != "0")
        .collect()
}
