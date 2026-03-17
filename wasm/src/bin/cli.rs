use anyhow::Context;
use clap::Parser;
use nrftw_wasm::document::parse_all_docs;
use nrftw_wasm::dumper::{force_dump_doc_serialized, serialized_content_bytes, try_force_dump_all_docs};
use nrftw_wasm::mutations::{apply_changes, materialize_doc_backrefs, ApplyChangesRequest, ItemDraft};
use nrftw_wasm::parser::content::{find_node, node_meta, ConcreteNode, ContentNode, ContentValue, PrimitiveValue};
use nrftw_wasm::parser::schema::{CerimalSchema, ClassMember, TypeDefMembers};
use nrftw_wasm::parser::types::{format_guid_le, CerimalTypeReference, WellKnownType};
use nrftw_wasm::parser::{content::children_of, schema::fmt_type_ref};
use serde_json::{json, Value};

const BROTHERS_KEEPERS_GUID: &str = "2015248703842407367";

#[derive(Parser)]
struct Args {
    /// The cerimal file to parse
    cerimal_file: String,
    /// Path to the zstd dictionary (optional; omit for uncompressed saves)
    #[arg(short, long)]
    dict_path: Option<String>,

    #[arg(long)]
    json: bool,

    /// Re-dump the parsed save to this output path
    #[arg(long)]
    dump_out: Option<String>,

    /// Force-reserialize only one document index and write the combined save to --dump-out
    #[arg(long)]
    dump_doc: Option<usize>,

    /// Compare original decompressed content bytes with the serializer output for one document
    #[arg(long)]
    dump_doc_content_diff: Option<usize>,

    /// Set the Brothers Keepers dagger level and write the mutated save to --dump-out
    #[arg(long)]
    brothers_keepers_level: Option<i64>,

    /// Materialize backrefs and re-dump the save to --dump-out
    #[arg(long)]
    repair_backrefs: bool,

    /// Path to catalog.db for GUID → asset name resolution (optional)
    #[arg(long)]
    catalog: Option<String>,
}

// ── Catalog lookup ────────────────────────────────────────────────────────────

struct CatalogEntry {
    name: String,
    display_name: String,
    script_type: Option<String>,
}

struct CatalogLookup {
    conn: rusqlite::Connection,
}

impl CatalogLookup {
    fn open(path: &str) -> Result<Self, String> {
        rusqlite::Connection::open(path)
            .map(|conn| Self { conn })
            .map_err(|e| e.to_string())
    }

    fn lookup(&self, guid: u64) -> Option<CatalogEntry> {
        self.conn
            .query_row(
                "SELECT name, display_name, script_type FROM assets WHERE asset_guid = ?1",
                [guid as i64],
                |row| {
                    Ok(CatalogEntry {
                        name: row.get(0)?,
                        display_name: row.get(1)?,
                        script_type: row.get(2)?,
                    })
                },
            )
            .ok()
    }
}

// ── JSON serialisation ────────────────────────────────────────────────────────

fn primitive_to_json(pv: &PrimitiveValue) -> Value {
    match pv {
        PrimitiveValue::Bool(v) => json!(v),
        PrimitiveValue::I64(v) => json!(v),
        PrimitiveValue::U64(v) => json!(v),
        PrimitiveValue::F64(v) | PrimitiveValue::Fp(v) | PrimitiveValue::Lfp(v) => json!(v),
        PrimitiveValue::Str(v) => json!(v),
        PrimitiveValue::Guid(b) => json!(format_guid_le(b)),
        PrimitiveValue::Bytes(b) => json!(b),
    }
}

fn concrete_value_to_json(
    node: &ConcreteNode,
    schema: &CerimalSchema,
    catalog: Option<&CatalogLookup>,
) -> Value {
    match &node.value {
        ContentValue::Primitive(pv) => {
            // Enrich AssetGuid with catalog metadata when available
            if matches!(
                &node.type_ref,
                CerimalTypeReference::Primitive(WellKnownType::AssetGuid)
            ) {
                if let PrimitiveValue::U64(guid_val) = pv {
                    if let Some(cat) = catalog {
                        if let Some(entry) = cat.lookup(*guid_val) {
                            return json!({
                                "$guid": guid_val.to_string(),
                                "name": entry.name,
                                "displayName": entry.display_name,
                                "type": entry.script_type,
                            });
                        }
                    }
                }
            }
            primitive_to_json(pv)
        }
        ContentValue::Composite(fields) => {
            let mut map = serde_json::Map::new();
            for (name, child) in fields {
                map.insert(name.clone(), node_to_json(child, schema, catalog));
            }
            Value::Object(map)
        }
        ContentValue::Array { elements, .. } => {
            Value::Array(elements.iter().map(|e| node_to_json(e, schema, catalog)).collect())
        }
        ContentValue::Enum(v) => {
            let CerimalTypeReference::Named { guid, .. } = &node.type_ref else {
                return json!({"$enum": v, "$error": "expected named type reference"});
            };
            let Some(&type_def_idx) = schema.guid_map.get(guid) else {
                return json!({"$enum": v, "$error": "type def not found"});
            };
            let TypeDefMembers::Enum(em) = &schema.type_defs[type_def_idx].members else {
                return json!({"$enum": v, "$error": "expected enum members"});
            };
            let variant_name = em
                .variants
                .iter()
                .find(|var| var.value == *v)
                .map(|var| var.name.as_str())
                .unwrap_or("unknown");
            json!(variant_name)
        }
    }
}

fn node_to_json(node: &ContentNode, schema: &CerimalSchema, catalog: Option<&CatalogLookup>) -> Value {
    match node {
        ContentNode::Null(_) => Value::Null,
        ContentNode::Backref { index, .. } => json!({"$ref": index}),
        ContentNode::Polymorphic {
            runtime_type,
            inner,
            ..
        } => {
            json!({"$type": fmt_type_ref(runtime_type, schema), "value": node_to_json(inner, schema, catalog)})
        }
        ContentNode::Concrete(n) => concrete_value_to_json(n, schema, catalog),
    }
}

fn schema_to_json(schema: &CerimalSchema) -> Value {
    schema.type_defs.iter().map(|td| {
    let members_json: Vec<Value> = match &td.members {
        TypeDefMembers::Class(members) => {
            members.iter().map(|m| {
                match m {
                    ClassMember::Field { name, type_ref} => json!({"field": name, "type": fmt_type_ref(type_ref, schema)}),
                    ClassMember::List { element_type } => json!({"list": fmt_type_ref(element_type, schema)}),
                    ClassMember::Dict { key_type, value_type } => json!({"dict_key": fmt_type_ref(key_type, schema), "dict_value": fmt_type_ref(value_type, schema)})
                }
            }).collect()
        },
        TypeDefMembers::StructLike(members) => {
            members.iter().map(|m|
                json!({"field": m.name, "type": fmt_type_ref(&m.type_ref, schema)})
            ).collect()
        },
        TypeDefMembers::Enum(em) => {
            em.variants.iter().map(|v| json!({"name": v.name, "value": v.value})).collect()
        }
    };
    json!({
        "name": td.name,
        "guid": format_guid_le(&td.guid),
        "kind": format!("{:?}", td.kind),
        "version": td.local_version,
        "members": members_json
        })
    }).collect()
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    validate_args(&args)?;

    let data = std::fs::read(&args.cerimal_file)
        .with_context(|| format!("failed to read cerimal file: {}", args.cerimal_file))?;
    let dict = args
        .dict_path
        .map(|p| std::fs::read(&p).with_context(|| format!("failed to read dict file: {p}")))
        .transpose()?
        .unwrap_or_default();

    let catalog = args
        .catalog
        .as_deref()
        .map(|p| CatalogLookup::open(p).map_err(|e| anyhow::anyhow!("failed to open catalog {p}: {e}")))
        .transpose()?;

    let mut docs = parse_all_docs(&data, &dict)
        .map_err(|e| anyhow::anyhow!("failed to parse save file: {e}"))?;

    let e = |s: String| anyhow::anyhow!(s);
    let handled_output_mode = if let Some(doc_idx) = args.dump_doc_content_diff {
        print_doc_content_diff(&docs, doc_idx).map_err(&e)?;
        true
    } else if args.repair_backrefs {
        let out_path = args
            .dump_out
            .as_deref()
            .context("--repair-backrefs requires --dump-out")?;
        for doc in docs.iter_mut() {
            materialize_doc_backrefs(doc).map_err(&e)?;
        }
        let dumped = try_force_dump_all_docs(&docs, &dict).map_err(&e)?;
        std::fs::write(out_path, dumped).context("write repaired save file")?;
        true
    } else if let Some(level) = args.brothers_keepers_level {
        let out_path = args
            .dump_out
            .as_deref()
            .context("--brothers-keepers-level requires --dump-out")?;
        let dumped = mutate_brothers_keepers_level(&mut docs, &dict, level).map_err(&e)?;
        std::fs::write(out_path, dumped).context("write dumped save file")?;
        true
    } else if let Some(doc_idx) = args.dump_doc {
        let out_path = args
            .dump_out
            .as_deref()
            .context("--dump-doc requires --dump-out")?;
        let dumped = force_dump_single_doc(&docs, &dict, doc_idx).map_err(&e)?;
        std::fs::write(out_path, dumped).context("write dumped save file")?;
        true
    } else if let Some(out_path) = &args.dump_out {
        let dumped = try_force_dump_all_docs(&docs, &dict).map_err(&e)?;
        std::fs::write(out_path, dumped).context("write dumped save file")?;
        true
    } else {
        false
    };

    if args.json {
        let json_docs: Vec<Value> = docs
            .iter()
            .map(|doc| {
                json!({
                    "schema": schema_to_json(&doc.schema),
                    "root_type": fmt_type_ref(&doc.schema.root_type_ref, &doc.schema),
                    "content": node_to_json(&doc.root, &doc.schema, catalog.as_ref()),
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&json_docs)?);
        return Ok(());
    }

    if handled_output_mode {
        return Ok(());
    }

    for (i, doc) in docs.iter().enumerate() {
        let root_type = fmt_type_ref(&doc.schema.root_type_ref, &doc.schema);
        println!("Doc {}: {}", i, root_type);
        let kids = children_of(&doc.root, "", &doc.schema);
        for c in kids {
            let val = c.meta.value.unwrap_or_default();
            let annotation = c.meta.guid.as_deref().and_then(|guid_str| {
                let cat = catalog.as_ref()?;
                let guid_val: u64 = guid_str.parse().ok()?;
                let entry = cat.lookup(guid_val)?;
                let type_label = entry.script_type.as_deref().unwrap_or("?");
                Some(format!(" → {} [{}]", entry.display_name, type_label))
            }).unwrap_or_default();
            println!("  {}: {} = {}{}", c.key, c.meta.type_str, val, annotation);
        }
    }

    Ok(())
}

fn validate_args(args: &Args) -> anyhow::Result<()> {
    let output_modes = [
        args.dump_out.is_some()
            && args.dump_doc.is_none()
            && args.brothers_keepers_level.is_none()
            && !args.repair_backrefs,
        args.dump_doc.is_some(),
        args.brothers_keepers_level.is_some(),
        args.repair_backrefs,
    ]
    .into_iter()
    .filter(|enabled| *enabled)
    .count();

    anyhow::ensure!(
        !(args.dump_doc_content_diff.is_some() && output_modes > 0),
        "--dump-doc-content-diff cannot be combined with --dump-out, --dump-doc, or --brothers-keepers-level"
    );
    anyhow::ensure!(
        !(args.dump_doc.is_some() && args.brothers_keepers_level.is_some()),
        "--dump-doc cannot be combined with --brothers-keepers-level"
    );
    anyhow::ensure!(
        !(args.repair_backrefs && (args.dump_doc.is_some() || args.brothers_keepers_level.is_some())),
        "--repair-backrefs cannot be combined with --dump-doc or --brothers-keepers-level"
    );
    anyhow::ensure!(
        !(args.dump_doc.is_some() && args.dump_out.is_none()),
        "--dump-doc requires --dump-out"
    );
    anyhow::ensure!(
        !(args.brothers_keepers_level.is_some() && args.dump_out.is_none()),
        "--brothers-keepers-level requires --dump-out"
    );
    anyhow::ensure!(
        !(args.repair_backrefs && args.dump_out.is_none()),
        "--repair-backrefs requires --dump-out"
    );
    Ok(())
}

fn mutate_brothers_keepers_level(
    docs: &mut [nrftw_wasm::document::ParsedDoc],
    dict: &[u8],
    level: i64,
) -> Result<Vec<u8>, String> {
    let (item_path, rarity, durability) = find_brothers_keepers(docs)?;

    apply_changes(
        docs,
        ApplyChangesRequest {
            stat_edits: vec![],
            item_deletes: vec![],
            item_edits: vec![ItemDraft {
                doc_idx: 1,
                item_path: Some(item_path),
                asset_guid: BROTHERS_KEEPERS_GUID.to_string(),
                level,
                rarity,
                durability,
                stack_count: 1,
                rune_guids: vec![],
                enchantment_guids: vec![],
            }],
            item_creates: vec![],
        },
        dict,
    )
}

fn find_brothers_keepers(
    docs: &[nrftw_wasm::document::ParsedDoc],
) -> Result<(String, i64, i64), String> {
    let doc = docs.get(1).ok_or_else(|| "CharacterContents doc not found".to_string())?;
    let inventory = find_node(&doc.root, "Inventory").ok_or_else(|| "Inventory not found".to_string())?;
    let child_count = node_meta(inventory, &doc.schema).child_count;

    for index in 0..child_count {
        let item_path = format!("Inventory[{index}]");
        let guid = node_value(doc, &format!("{item_path}.Item.ItemData.Id"))?;
        if guid != BROTHERS_KEEPERS_GUID {
            continue;
        }

        let rarity = node_value(doc, &format!("{item_path}.Item.Rarity"))?
            .parse::<i64>()
            .map_err(|e| format!("parse rarity: {e}"))?;
        let durability = node_value(doc, &format!("{item_path}.Item.Durability.Durability"))?
            .parse::<i64>()
            .map_err(|e| format!("parse durability: {e}"))?;
        return Ok((item_path, rarity, durability));
    }

    Err("Brothers Keepers item not found in inventory".to_string())
}

fn node_value(doc: &nrftw_wasm::document::ParsedDoc, path: &str) -> Result<String, String> {
    let node = find_node(&doc.root, path).ok_or_else(|| format!("missing node: {path}"))?;
    node_meta(node, &doc.schema)
        .value
        .ok_or_else(|| format!("missing value: {path}"))
}

fn force_dump_single_doc(
    docs: &[nrftw_wasm::document::ParsedDoc],
    dict: &[u8],
    doc_idx: usize,
) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    for (idx, doc) in docs.iter().enumerate() {
        if idx == doc_idx {
            out.extend_from_slice(&force_dump_doc_serialized(doc, dict)?);
        } else {
            out.extend_from_slice(&doc.raw_bytes);
        }
    }
    if doc_idx >= docs.len() {
        return Err(format!("doc {} not found", doc_idx));
    }
    Ok(out)
}

fn print_doc_content_diff(
    docs: &[nrftw_wasm::document::ParsedDoc],
    doc_idx: usize,
) -> Result<(), String> {
    let doc = docs
        .get(doc_idx)
        .ok_or_else(|| format!("doc {} not found", doc_idx))?;
    let serialized = serialized_content_bytes(doc)?;
    let original = &doc.content_bytes;

    println!("doc: {}", doc_idx);
    println!("original_len: {}", original.len());
    println!("serialized_len: {}", serialized.len());

    let original_table = parse_backref_table(original)?;
    let serialized_table = parse_backref_table(&serialized)?;
    println!(
        "original_backrefs: count={}, table_len={}, entries={:?}",
        original_table.entries.len(),
        original_table.header_len,
        original_table.entries
    );
    println!(
        "serialized_backrefs: count={}, table_len={}, entries={:?}",
        serialized_table.entries.len(),
        serialized_table.header_len,
        serialized_table.entries
    );

    let mut first_diff = None;
    let mut mismatch_count = 0usize;
    let max_len = original.len().max(serialized.len());
    for idx in 0..max_len {
        let lhs = original.get(idx);
        let rhs = serialized.get(idx);
        if lhs != rhs {
            mismatch_count += 1;
            if first_diff.is_none() {
                first_diff = Some(idx);
            }
        }
    }

    match first_diff {
        Some(offset) => {
            println!("first_diff: {}", offset);
            println!("mismatch_count: {}", mismatch_count);
            let start = offset.saturating_sub(16);
            let end = (offset + 16).min(max_len.saturating_sub(1));
            println!("original_hex: {}", format_hex_window(original, start, end));
            println!("serialized_hex: {}", format_hex_window(&serialized, start, end));
        }
        None => {
            println!("first_diff: none");
            println!("mismatch_count: 0");
        }
    }

    let original_body = &original[original_table.header_len..];
    let serialized_body = &serialized[serialized_table.header_len..];
    let (body_first_diff, body_mismatch_count) = diff_stats(original_body, serialized_body);
    println!("body_original_len: {}", original_body.len());
    println!("body_serialized_len: {}", serialized_body.len());
    match body_first_diff {
        Some(offset) => {
            println!("body_first_diff: {}", offset);
            println!("body_mismatch_count: {}", body_mismatch_count);
            let start = offset.saturating_sub(16);
            let end = (offset + 16).min(original_body.len().max(serialized_body.len()).saturating_sub(1));
            println!("body_original_hex: {}", format_hex_window(original_body, start, end));
            println!("body_serialized_hex: {}", format_hex_window(serialized_body, start, end));
        }
        None => {
            println!("body_first_diff: none");
            println!("body_mismatch_count: 0");
        }
    }

    Ok(())
}

struct BackrefTable {
    header_len: usize,
    entries: Vec<u32>,
}

fn parse_backref_table(bytes: &[u8]) -> Result<BackrefTable, String> {
    let (count, mut pos) = read_7bit(bytes, 0)?;
    let mut entries = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let (entry, next_pos) = read_7bit(bytes, pos)?;
        entries.push(entry);
        pos = next_pos;
    }
    Ok(BackrefTable {
        header_len: pos,
        entries,
    })
}

fn read_7bit(bytes: &[u8], start: usize) -> Result<(u32, usize), String> {
    let mut result = 0u32;
    let mut shift = 0;
    let mut pos = start;
    loop {
        let byte = *bytes
            .get(pos)
            .ok_or_else(|| format!("unexpected EOF reading 7-bit int at {}", start))?;
        pos += 1;
        result |= ((byte & 0x7f) as u32) << shift;
        if byte & 0x80 == 0 {
            return Ok((result, pos));
        }
        shift += 7;
        if shift >= 35 {
            return Err(format!("invalid 7-bit int at {}", start));
        }
    }
}

fn diff_stats(lhs: &[u8], rhs: &[u8]) -> (Option<usize>, usize) {
    let mut first_diff = None;
    let mut mismatch_count = 0usize;
    let max_len = lhs.len().max(rhs.len());
    for idx in 0..max_len {
        let left = lhs.get(idx);
        let right = rhs.get(idx);
        if left != right {
            mismatch_count += 1;
            if first_diff.is_none() {
                first_diff = Some(idx);
            }
        }
    }
    (first_diff, mismatch_count)
}

fn format_hex_window(bytes: &[u8], start: usize, end: usize) -> String {
    (start..=end)
        .map(|idx| {
            bytes.get(idx)
                .map(|byte| format!("{idx:04x}:{byte:02x}"))
                .unwrap_or_else(|| format!("{idx:04x}:--"))
        })
        .collect::<Vec<_>>()
        .join(" ")
}
