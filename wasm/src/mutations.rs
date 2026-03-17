use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::document::ParsedDoc;
use crate::dumper::{
    compress_content, serialized_content_bytes, try_dump_all_docs_with_dirty_and_compressor,
};
use crate::parser::content::{find_node, ConcreteNode, ContentNode, ContentValue, PrimitiveValue};
use crate::parser::schema::{
    fmt_type_ref, resolve_type_arg, CerimalSchema, CerimalTypeDefinition, ClassMember,
    TypeDefMembers,
};
use crate::parser::types::{CerimalTypeReference, CompressionType, WellKnownType};

/// Sentinel value for unequipped inventory slots (u32::MAX as i64).
pub(crate) const UNEQUIPPED_SLOT: i64 = 4_294_967_295;

// Game rarity tiers — determines enchantment capacity:
//   Normal (0): 0 enchantments
//   Rare   (1): 1–3 enchantments
//   Cursed (2): 4+ enchantments
//   Unique (3): fixed loadout, not player-selectable
const WHITE_RARITY: i64 = 0;
const BLUE_RARITY: i64 = 1;
const PURPLE_RARITY: i64 = 2;
const GOLD_RARITY: i64 = 3;
const DEFAULT_CREATE_DURABILITY: i64 = 100;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StatEdit {
    pub doc_idx: usize,
    pub path: String,
    pub value: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ItemDraft {
    pub doc_idx: usize,
    pub item_path: Option<String>,
    pub asset_guid: String,
    pub level: i64,
    pub rarity: i64,
    pub durability: i64,
    pub stack_count: i64,
    pub rune_guids: Vec<String>,
    pub enchantment_guids: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ItemDelete {
    pub doc_idx: usize,
    pub item_path: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ApplyChangesRequest {
    pub stat_edits: Vec<StatEdit>,
    pub item_deletes: Vec<ItemDelete>,
    pub item_edits: Vec<ItemDraft>,
    pub item_creates: Vec<ItemDraft>,
}

pub fn apply_changes(
    docs: &mut [ParsedDoc],
    req: ApplyChangesRequest,
    dict_bytes: &[u8],
) -> Result<Vec<u8>, String> {
    apply_changes_with_compressor(docs, req, |comp_type, content| {
        compress_content(comp_type, content, dict_bytes)
    })
}

pub fn apply_changes_with_compressor(
    docs: &mut [ParsedDoc],
    req: ApplyChangesRequest,
    compress: impl Fn(crate::parser::types::CompressionType, &[u8]) -> Result<Vec<u8>, String>,
) -> Result<Vec<u8>, String> {
    for doc in docs.iter_mut() {
        materialize_doc_backrefs(doc)?;
    }

    let mut dirty = BTreeSet::new();
    let inventory_templates = collect_inventory_templates(docs);

    for stat in req.stat_edits {
        let mirrors_character_contents = docs
            .get(stat.doc_idx)
            .map(is_character_metadata_doc)
            .ok_or_else(|| format!("doc {} not found", stat.doc_idx))?;

        let doc = docs
            .get_mut(stat.doc_idx)
            .ok_or_else(|| format!("doc {} not found", stat.doc_idx))?;
        apply_stat_edit(doc, &stat.path, &stat.value)?;
        dirty.insert(stat.doc_idx);

        if mirrors_character_contents {
            sync_stat_to_character_contents(docs, &stat.path, &stat.value, &mut dirty)?;
        }
    }

    for item in req.item_deletes {
        let doc = docs
            .get_mut(item.doc_idx)
            .ok_or_else(|| format!("doc {} not found", item.doc_idx))?;
        apply_item_delete(doc, &item.item_path)?;
        dirty.insert(item.doc_idx);
    }

    for item in req.item_edits {
        let doc = docs
            .get_mut(item.doc_idx)
            .ok_or_else(|| format!("doc {} not found", item.doc_idx))?;
        let item_path = item
            .item_path
            .as_deref()
            .ok_or_else(|| "item edit missing item_path".to_string())?;
        apply_item_edit(doc, item_path, &item)?;
        dirty.insert(item.doc_idx);
    }

    for item in req.item_creates {
        let doc = docs
            .get_mut(item.doc_idx)
            .ok_or_else(|| format!("doc {} not found", item.doc_idx))?;
        apply_item_create(doc, &item, inventory_templates.get(&item.doc_idx))?;
        dirty.insert(item.doc_idx);
    }

    sync_metadata_contents_checksums(docs, &mut dirty, &compress)?;

    try_dump_all_docs_with_dirty_and_compressor(docs, &dirty, compress)
}

fn is_character_metadata_doc(doc: &ParsedDoc) -> bool {
    fmt_type_ref(&doc.schema.root_type_ref, &doc.schema) == "Quantum.CharacterMetadata"
}

fn is_character_contents_doc(doc: &ParsedDoc) -> bool {
    fmt_type_ref(&doc.schema.root_type_ref, &doc.schema) == "Quantum.CharacterContents"
}

fn sync_stat_to_character_contents(
    docs: &mut [ParsedDoc],
    path: &str,
    value: &str,
    dirty: &mut BTreeSet<usize>,
) -> Result<(), String> {
    let mut updated = false;

    for (idx, doc) in docs.iter_mut().enumerate() {
        if !is_character_contents_doc(doc) {
            continue;
        }

        let changed = match path {
            "Level" | "XP" | "Gold" => {
                apply_stat_edit(doc, path, value)?;
                true
            }
            "Health" => set_character_contents_attribute(doc, "Health", value)?,
            "Stamina" => set_character_contents_attribute(doc, "Stamina", value)?,
            "Strength" => set_character_contents_attribute(doc, "Strength", value)?,
            "Dexterity" => set_character_contents_attribute(doc, "Dexterity", value)?,
            "Intelligence" => set_character_contents_attribute(doc, "Intelligence", value)?,
            "Faith" => set_character_contents_attribute(doc, "Faith", value)?,
            "Focus" => set_character_contents_attribute(doc, "Focus", value)?,
            "Load" => set_character_contents_attribute(doc, "ItemLoad", value)?,
            p if p.starts_with("Customization.") => {
                apply_stat_edit(doc, path, value)?;
                true
            }
            _ => false,
        };

        if changed {
            dirty.insert(idx);
            updated = true;
        }
    }

    let is_mirrored_path = path.starts_with("Customization.")
        || matches!(
            path,
            "Level"
                | "XP"
                | "Gold"
                | "Health"
                | "Stamina"
                | "Strength"
                | "Dexterity"
                | "Intelligence"
                | "Faith"
                | "Focus"
                | "Load"
        );
    if is_mirrored_path && !updated {
        return Err(format!(
            "no Quantum.CharacterContents doc found to mirror '{}'",
            path
        ));
    }

    Ok(())
}

fn apply_stat_edit(doc: &mut ParsedDoc, path: &str, value: &str) -> Result<(), String> {
    let node =
        find_node_mut(&mut doc.root, path).ok_or_else(|| format!("field '{}' not found", path))?;
    set_leaf_value(node, value)
}

fn set_character_contents_attribute(
    doc: &mut ParsedDoc,
    key: &str,
    value: &str,
) -> Result<bool, String> {
    let attributes = find_node_mut(&mut doc.root, "Attributes.$dict0")
        .ok_or_else(|| "CharacterContents.Attributes.$dict0 not found".to_string())?;
    let fields = composite_fields_mut(attributes)?;
    let value_field_name = find_dict_value_field_name(fields, key, &doc.schema)
        .ok_or_else(|| format!("CharacterContents attribute '{}' not found", key))?;
    let value_field = find_field_mut(fields, &value_field_name)?;
    set_leaf_value(value_field, value)?;
    Ok(true)
}

fn sync_metadata_contents_checksums(
    docs: &mut [ParsedDoc],
    dirty: &mut BTreeSet<usize>,
    compress: &impl Fn(CompressionType, &[u8]) -> Result<Vec<u8>, String>,
) -> Result<(), String> {
    let mut checksum_updates = Vec::new();

    for &idx in dirty.iter() {
        let Some(doc) = docs.get(idx) else {
            continue;
        };
        if !is_character_contents_doc(doc) {
            continue;
        }

        let content = serialized_content_bytes(doc)?;
        let compressed = compress(doc.comp_type, &content)?;
        let new_checksum = xxhash_rust::xxh64::xxh64(
            &[doc.schema_bytes.as_slice(), compressed.as_slice()].concat(),
            0,
        );
        checksum_updates.push((idx, doc.header.checksum, new_checksum));
    }

    if checksum_updates.is_empty() {
        return Ok(());
    }

    for (meta_idx, doc) in docs.iter_mut().enumerate() {
        if !is_character_metadata_doc(doc) {
            continue;
        }

        let current_checksum = get_leaf_u64(&doc.root, "Common.ContentsChecksum")?;
        let pending_checksum = get_leaf_u64(&doc.root, "Common.PendingContentsChecksum")?;
        let Some((_, previous_checksum, new_checksum)) =
            checksum_updates.iter().find(|(_, old_checksum, _)| {
                *old_checksum == current_checksum || *old_checksum == pending_checksum
            })
        else {
            continue;
        };

        apply_stat_edit(doc, "Common.ContentsChecksum", &new_checksum.to_string())?;
        apply_stat_edit(doc, "Common.PendingContentsChecksum", &new_checksum.to_string())?;
        apply_stat_edit(
            doc,
            "Common.PreviousContentsChecksum",
            &previous_checksum.to_string(),
        )?;
        dirty.insert(meta_idx);
    }

    Ok(())
}

fn apply_item_edit(doc: &mut ParsedDoc, item_path: &str, draft: &ItemDraft) -> Result<(), String> {
    let entry = find_node_mut(&mut doc.root, item_path)
        .ok_or_else(|| format!("item '{}' not found", item_path))?;
    apply_item_draft_to_entry(entry, &doc.schema, draft, false)
}

fn apply_item_delete(doc: &mut ParsedDoc, item_path: &str) -> Result<(), String> {
    let inventory = find_node_mut(&mut doc.root, "Inventory")
        .ok_or_else(|| "Inventory not found".to_string())?;
    let (dims, elements) = array_parts_mut(inventory)?;
    let (_, Some(index)) = parse_head(item_path) else {
        return Err(format!("invalid inventory path '{}'", item_path));
    };
    if index >= elements.len() {
        return Err(format!("item '{}' not found", item_path));
    }
    elements.remove(index);
    sync_array_dims(dims, elements.len());
    Ok(())
}

fn collect_inventory_templates(docs: &[ParsedDoc]) -> BTreeMap<usize, ContentNode> {
    docs.iter()
        .enumerate()
        .filter_map(|(idx, doc)| {
            let template = find_item_template(doc)?;
            let materialized = materialize_subtree_backrefs(template, &doc.root).ok()?;
            Some((idx, materialized))
        })
        .collect()
}

fn apply_item_create(
    doc: &mut ParsedDoc,
    draft: &ItemDraft,
    template_override: Option<&ContentNode>,
) -> Result<(), String> {
    let template_entry = if let Some(template) = find_item_template(doc) {
        materialize_subtree_backrefs(template, &doc.root)?
    } else {
        template_override
            .cloned()
            .ok_or_else(|| "could not find inventory template item".to_string())?
    };

    let mut new_entry = template_entry;
    apply_item_draft_to_entry(&mut new_entry, &doc.schema, draft, true)?;

    let inventory = find_node_mut(&mut doc.root, "Inventory")
        .ok_or_else(|| "Inventory not found".to_string())?;
    let (dims, elements) = array_parts_mut(inventory)?;
    elements.push(new_entry);
    sync_array_dims(dims, elements.len());
    Ok(())
}

fn apply_item_draft_to_entry(
    entry: &mut ContentNode,
    schema: &CerimalSchema,
    draft: &ItemDraft,
    force_unequipped_slot: bool,
) -> Result<(), String> {
    let effective_rarity = normalize_rarity_for_enchantments(draft.rarity, draft.enchantment_guids.len());
    let entry_fields = composite_fields_mut(entry)?;
    if force_unequipped_slot {
        set_enum(find_field_mut(entry_fields, "Slot")?, UNEQUIPPED_SLOT)?;
    }

    let item = find_field_mut(entry_fields, "Item")?;
    let item_fields = composite_fields_mut(item)?;

    set_leaf_numeric(find_field_mut(item_fields, "Level")?, draft.level)?;
    set_leaf_numeric(find_field_mut(item_fields, "CreationLevel")?, draft.level)?;
    set_enum(find_field_mut(item_fields, "Rarity")?, effective_rarity)?;

    let durability = find_field_mut(item_fields, "Durability")?;
    let durability_fields = composite_fields_mut(durability)?;
    let effective_durability = if force_unequipped_slot {
        DEFAULT_CREATE_DURABILITY
    } else {
        draft.durability
    };
    set_leaf_numeric(
        find_field_mut(durability_fields, "Durability")?,
        effective_durability,
    )?;

    let item_data = find_field_mut(item_fields, "ItemData")?;
    let item_data_fields = composite_fields_mut(item_data)?;
    set_leaf_u64(find_field_mut(item_data_fields, "Id")?, parse_u64(&draft.asset_guid)?)?;

    let stack = find_field_mut(item_fields, "Stack")?;
    let stack_fields = composite_fields_mut(stack)?;
    set_leaf_numeric(
        find_field_mut(stack_fields, "Count")?,
        draft.stack_count.max(1),
    )?;

    if force_unequipped_slot || !draft.rune_guids.is_empty() {
        let weapon = ensure_named_field(item, "Weapon", schema)?;
        apply_runes(weapon, schema, &draft.rune_guids)?;
    }

    let has_existing_enchantments = current_enchantment_count(item) > 0;
    if force_unequipped_slot || !draft.enchantment_guids.is_empty() || has_existing_enchantments {
        let enchantment = ensure_named_field(item, "Enchantment", schema)?;
        apply_enchantments(enchantment, schema, &draft.enchantment_guids)?;
    }

    Ok(())
}

fn normalize_rarity_for_enchantments(rarity: i64, enchantment_count: usize) -> i64 {
    let minimum_rarity = match enchantment_count {
        0 => WHITE_RARITY,
        1..=3 => BLUE_RARITY,
        _ => PURPLE_RARITY,
    };

    if rarity == GOLD_RARITY {
        GOLD_RARITY
    } else {
        rarity.max(minimum_rarity)
    }
}

fn apply_enchantments(
    enchantment: &mut ContentNode,
    schema: &CerimalSchema,
    guids: &[String],
) -> Result<(), String> {
    {
        let has_value = ensure_named_field(enchantment, "HasValue", schema)?;
        set_bool(has_value, !guids.is_empty())?;
    }

    let value_type = named_field_type(enchantment.type_ref().clone(), "value", schema)?
        .ok_or_else(|| "could not resolve enchantment value field type".to_string())?;
    let list_type = named_field_type(value_type, "Enchantment", schema)?
        .ok_or_else(|| "could not resolve enchantment list field type".to_string())?;
    let (dims, elements) = ensure_enchantment_list(enchantment, schema)?;
    elements.clear();

    for guid in guids {
        let mut entry = build_default_node(array_element_type(&list_type)?, schema, None)?;
        set_enchantment_guid(&mut entry, guid)?;
        elements.push(entry);
    }
    sync_array_dims(dims, elements.len());

    Ok(())
}

fn current_enchantment_count(item: &ContentNode) -> usize {
    let Some(list) = find_node(item, "Enchantment.value.Enchantment") else {
        return 0;
    };
    let ContentNode::Concrete(node) = list.unwrap_poly() else {
        return 0;
    };
    let ContentValue::Array { elements, .. } = &node.value else {
        return 0;
    };
    elements.len()
}

fn apply_runes(
    weapon: &mut ContentNode,
    schema: &CerimalSchema,
    guids: &[String],
) -> Result<(), String> {
    {
        let has_value = ensure_named_field(weapon, "HasValue", schema)?;
        set_bool(has_value, true)?;
    }

    let (list_type, dims, elements) = ensure_rune_slot_list(weapon, schema)?;
    let element_type = array_element_type(&list_type)?.clone();

    // Expand the slot array if there are more runes than existing slots (e.g. when
    // creating a weapon from a template that has no rune slots).
    while elements.len() < guids.len() {
        elements.push(build_default_node(&element_type, schema, None)?);
    }

    for (index, slot) in elements.iter_mut().enumerate() {
        let guid = guids.get(index).map(|value| value.as_str()).unwrap_or("0");
        set_rune_guid(slot, guid)?;
    }

    sync_array_dims(dims, elements.len());
    Ok(())
}

fn set_enchantment_guid(node: &mut ContentNode, guid: &str) -> Result<(), String> {
    let fields = composite_fields_mut(node)?;
    let asset = find_field_mut(fields, "Asset")?;
    let asset_fields = composite_fields_mut(asset)?;
    set_leaf_u64(find_field_mut(asset_fields, "Id")?, parse_u64(guid)?)?;
    Ok(())
}

fn set_rune_guid(node: &mut ContentNode, guid: &str) -> Result<(), String> {
    let fields = composite_fields_mut(node)?;
    let rune_data = find_field_mut(fields, "RuneDataId")?;
    let rune_data_fields = composite_fields_mut(rune_data)?;
    set_leaf_u64(find_field_mut(rune_data_fields, "Id")?, parse_u64(guid)?)?;
    Ok(())
}

fn find_item_template(doc: &ParsedDoc) -> Option<&ContentNode> {
    let inventory = find_node(&doc.root, "Inventory")?;
    let actual = inventory.unwrap_poly();
    let ContentNode::Concrete(node) = actual else {
        return None;
    };
    let ContentValue::Array { elements, .. } = &node.value else {
        return None;
    };

    elements.iter().find(|entry| {
        let Some(slot) = find_node(entry, "Slot") else {
            return false;
        };
        matches!(slot.unwrap_poly(), ContentNode::Concrete(node) if matches!(node.value, ContentValue::Enum(UNEQUIPPED_SLOT)))
    }).or_else(|| elements.first())
}

fn materialize_subtree_backrefs(
    subtree: &ContentNode,
    root: &ContentNode,
) -> Result<ContentNode, String> {
    let backrefs = collect_registered_backrefs(root);
    let mut cache = BTreeMap::new();
    clone_materialized_node(subtree, &backrefs, &mut cache)
}

pub fn materialize_doc_backrefs(doc: &mut ParsedDoc) -> Result<(), String> {
    doc.root = materialize_subtree_backrefs(&doc.root, &doc.root)?;
    Ok(())
}

fn collect_registered_backrefs(root: &ContentNode) -> Vec<ContentNode> {
    let mut out = Vec::new();
    collect_registered_backrefs_inner(root, &mut out);
    out
}

fn collect_registered_backrefs_inner(node: &ContentNode, out: &mut Vec<ContentNode>) {
    match node {
        ContentNode::Null(_) | ContentNode::Backref { .. } => {}
        ContentNode::Polymorphic { inner, .. } => collect_registered_backrefs_inner(inner, out),
        ContentNode::Concrete(concrete) => {
            match &concrete.value {
                ContentValue::Primitive(_) | ContentValue::Enum(_) => {}
                ContentValue::Composite(fields) => {
                    for (_, child) in fields {
                        collect_registered_backrefs_inner(child, out);
                    }
                }
                ContentValue::Array { elements, .. } => {
                    for child in elements {
                        collect_registered_backrefs_inner(child, out);
                    }
                }
            }

            if concrete.register_in_backrefs {
                out.push(node.clone());
            }
        }
    }
}

fn clone_materialized_node(
    node: &ContentNode,
    backrefs: &[ContentNode],
    cache: &mut BTreeMap<u32, ContentNode>,
) -> Result<ContentNode, String> {
    match node {
        ContentNode::Null(type_ref) => Ok(ContentNode::Null(type_ref.clone())),
        ContentNode::Backref { index, .. } => {
            if let Some(existing) = cache.get(index) {
                return Ok(existing.clone());
            }

            let target = backrefs
                .get(*index as usize)
                .ok_or_else(|| format!("backref index {} out of range during materialize", index))?;
            let materialized = clone_materialized_node(target, backrefs, cache)?;
            cache.insert(*index, materialized.clone());
            Ok(materialized)
        }
        ContentNode::Polymorphic {
            declared_type,
            runtime_type,
            inner,
        } => Ok(ContentNode::Polymorphic {
            declared_type: declared_type.clone(),
            runtime_type: runtime_type.clone(),
            inner: Box::new(clone_materialized_node(inner, backrefs, cache)?),
        }),
        ContentNode::Concrete(concrete) => {
            let value = match &concrete.value {
                ContentValue::Primitive(value) => ContentValue::Primitive(value.clone()),
                ContentValue::Enum(value) => ContentValue::Enum(*value),
                ContentValue::Composite(fields) => ContentValue::Composite(
                    fields
                        .iter()
                        .map(|(name, child)| {
                            Ok((
                                name.clone(),
                                clone_materialized_node(child, backrefs, cache)?,
                            ))
                        })
                        .collect::<Result<Vec<_>, String>>()?,
                ),
                ContentValue::Array { dims, elements } => ContentValue::Array {
                    dims: dims.clone(),
                    elements: elements
                        .iter()
                        .map(|child| clone_materialized_node(child, backrefs, cache))
                        .collect::<Result<Vec<_>, String>>()?,
                },
            };

            Ok(ContentNode::Concrete(Box::new(ConcreteNode {
                type_ref: concrete.type_ref.clone(),
                value,
                node_start: 0,
                value_start: 0,
                value_end: 0,
                register_in_backrefs: false,
            })))
        }
    }
}

fn find_node_mut<'a>(node: &'a mut ContentNode, path: &str) -> Option<&'a mut ContentNode> {
    if path.is_empty() {
        return Some(node);
    }

    let actual = unwrap_poly_mut(node);
    let (head, rest_opt) = match path.find('.') {
        Some(dot_pos) => (&path[..dot_pos], Some(&path[dot_pos + 1..])),
        None => (path, None),
    };
    let (field_name, idx) = parse_head(head);

    match actual {
        ContentNode::Concrete(node) => match &mut node.value {
            ContentValue::Composite(fields) => {
                let child = &mut fields.iter_mut().find(|(key, _)| key == field_name)?.1;
                let child = if let Some(index) = idx {
                    let ContentNode::Concrete(node) = unwrap_poly_mut(child) else {
                        return None;
                    };
                    let ContentValue::Array { elements, .. } = &mut node.value else {
                        return None;
                    };
                    elements.get_mut(index)?
                } else {
                    child
                };
                match rest_opt {
                    Some(rest) => find_node_mut(child, rest),
                    None => Some(child),
                }
            }
            ContentValue::Array { elements, .. } => {
                let index = field_name
                    .trim_matches(|c| c == '[' || c == ']')
                    .parse::<usize>()
                    .ok()?;
                let child = elements.get_mut(index)?;
                match rest_opt {
                    Some(rest) => find_node_mut(child, rest),
                    None => Some(child),
                }
            }
            _ => None,
        },
        _ => None,
    }
}

fn unwrap_poly_mut(node: &mut ContentNode) -> &mut ContentNode {
    let mut current = node;
    loop {
        match current {
            ContentNode::Polymorphic { inner, .. } => current = inner.as_mut(),
            _ => return current,
        }
    }
}

fn parse_head(head: &str) -> (&str, Option<usize>) {
    if let Some(bracket) = head.find('[') {
        let name = &head[..bracket];
        let idx_str = head[bracket + 1..].trim_end_matches(']');
        (name, idx_str.parse().ok())
    } else {
        (head, None)
    }
}

fn composite_fields_mut(node: &mut ContentNode) -> Result<&mut Vec<(String, ContentNode)>, String> {
    let actual = unwrap_poly_mut(node);
    let ContentNode::Concrete(node) = actual else {
        return Err("expected concrete composite node".to_string());
    };
    let ContentValue::Composite(fields) = &mut node.value else {
        return Err("expected composite value".to_string());
    };
    Ok(fields)
}

fn array_parts_mut(
    node: &mut ContentNode,
) -> Result<(&mut Vec<usize>, &mut Vec<ContentNode>), String> {
    let actual = unwrap_poly_mut(node);
    let ContentNode::Concrete(node) = actual else {
        return Err("expected concrete array node".to_string());
    };
    let ContentValue::Array { dims, elements } = &mut node.value else {
        return Err("expected array value".to_string());
    };
    Ok((dims, elements))
}

fn ensure_enchantment_list<'a>(
    enchantment: &'a mut ContentNode,
    schema: &CerimalSchema,
) -> Result<(&'a mut Vec<usize>, &'a mut Vec<ContentNode>), String> {
    let value = ensure_named_field(enchantment, "value", schema)?;
    let list_type = named_field_type(value.type_ref().clone(), "Enchantment", schema)?
        .ok_or_else(|| "could not resolve enchantment list field type".to_string())?;
    let list_node = ensure_named_field(value, "Enchantment", schema)?;
    if !matches!(
        unwrap_poly_mut(list_node),
        ContentNode::Concrete(node) if matches!(node.value, ContentValue::Array { .. })
    ) {
        *list_node = build_default_node(&list_type, schema, None)?;
    }
    array_parts_mut(list_node)
}

fn ensure_rune_slot_list<'a>(
    weapon: &'a mut ContentNode,
    schema: &CerimalSchema,
) -> Result<(CerimalTypeReference, &'a mut Vec<usize>, &'a mut Vec<ContentNode>), String> {
    let value = ensure_named_field(weapon, "value", schema)?;
    let list_type = named_field_type(value.type_ref().clone(), "RuneSlots", schema)?
        .ok_or_else(|| "could not resolve rune slot list field type".to_string())?;
    let list_node = ensure_named_field(value, "RuneSlots", schema)?;
    if !matches!(
        unwrap_poly_mut(list_node),
        ContentNode::Concrete(node) if matches!(node.value, ContentValue::Array { .. })
    ) {
        *list_node = build_default_node(&list_type, schema, None)?;
    }
    let (dims, elements) = array_parts_mut(list_node)?;
    Ok((list_type, dims, elements))
}

fn ensure_named_field<'a>(
    node: &'a mut ContentNode,
    field_name: &str,
    schema: &CerimalSchema,
) -> Result<&'a mut ContentNode, String> {
    let actual = unwrap_poly_mut(node);
    let ContentNode::Concrete(concrete) = actual else {
        return Err(format!("expected concrete composite node for '{}'", field_name));
    };
    let parent_type = concrete.type_ref.clone();
    let ContentValue::Composite(fields) = &mut concrete.value else {
        return Err(format!("expected composite value for '{}'", field_name));
    };
    if let Some(pos) = fields.iter().position(|(name, _)| name == field_name) {
        return Ok(&mut fields[pos].1);
    }

    let field_type = named_field_type(parent_type, field_name, schema)?
        .ok_or_else(|| format!("field '{}' not declared on parent type", field_name))?;
    fields.push((
        field_name.to_string(),
        build_default_node(&field_type, schema, None)?,
    ));
    Ok(&mut fields.last_mut().expect("field was just pushed").1)
}

fn named_field_type(
    type_ref: CerimalTypeReference,
    field_name: &str,
    schema: &CerimalSchema,
) -> Result<Option<CerimalTypeReference>, String> {
    named_field_type_with_args(&type_ref, field_name, schema, None)
}

fn named_field_type_with_args(
    type_ref: &CerimalTypeReference,
    field_name: &str,
    schema: &CerimalSchema,
    inherited_args: Option<&[CerimalTypeReference]>,
) -> Result<Option<CerimalTypeReference>, String> {
    let resolved = resolve_type_arg(type_ref, inherited_args);
    let CerimalTypeReference::Named { guid, type_args } = resolved else {
        return Ok(None);
    };
    let Some(type_idx) = schema.guid_map.get(guid).copied() else {
        return Ok(None);
    };
    let typedef = &schema.type_defs[type_idx];
    let effective_args = if !type_args.is_empty() {
        Some(type_args.as_slice())
    } else {
        inherited_args
    };

    if let Some(base_info) = &typedef.base_info {
        if base_info.has_base {
            if let Some(base_ref) = &base_info.base_type_ref {
                if let Some(found) =
                    named_field_type_with_args(base_ref, field_name, schema, effective_args)?
                {
                    return Ok(Some(found));
                }
            }
        }
    }

    let members = match &typedef.members {
        TypeDefMembers::Class(members) => members,
        TypeDefMembers::StructLike(members) => {
            for member in members {
                if member.name == field_name {
                    return Ok(Some(resolve_type_arg(&member.type_ref, effective_args).clone()));
                }
            }
            return Ok(None);
        }
        TypeDefMembers::Enum(_) => return Ok(None),
    };

    for member in members {
        if let ClassMember::Field { name, type_ref } = member {
            if name == field_name {
                return Ok(Some(resolve_type_arg(type_ref, effective_args).clone()));
            }
        }
    }
    Ok(None)
}

fn build_default_node(
    type_ref: &CerimalTypeReference,
    schema: &CerimalSchema,
    type_args: Option<&[CerimalTypeReference]>,
) -> Result<ContentNode, String> {
    let resolved = resolve_type_arg(type_ref, type_args);
    let value = build_default_value(resolved, schema, type_args)?;
    Ok(ContentNode::Concrete(Box::new(ConcreteNode {
        type_ref: resolved.clone(),
        value,
        node_start: 0,
        value_start: 0,
        value_end: 0,
        register_in_backrefs: false,
    })))
}

fn build_default_value(
    type_ref: &CerimalTypeReference,
    schema: &CerimalSchema,
    type_args: Option<&[CerimalTypeReference]>,
) -> Result<ContentValue, String> {
    let resolved = resolve_type_arg(type_ref, type_args);
    match resolved {
        CerimalTypeReference::Primitive(wkt) => Ok(ContentValue::Primitive(default_primitive(*wkt))),
        CerimalTypeReference::Array { rank, .. } => Ok(ContentValue::Array {
            dims: vec![0; *rank as usize],
            elements: Vec::new(),
        }),
        CerimalTypeReference::TypeArgument { .. } => {
            unreachable!("resolved type argument should not remain unresolved")
        }
        CerimalTypeReference::Named { guid, type_args: named_args } => {
            let type_idx = schema
                .guid_map
                .get(guid)
                .copied()
                .ok_or_else(|| "unknown type GUID in default builder".to_string())?;
            let typedef = &schema.type_defs[type_idx];
            let effective_args = if !named_args.is_empty() {
                Some(named_args.as_slice())
            } else {
                type_args
            };
            match &typedef.members {
                TypeDefMembers::Enum(_) => Ok(ContentValue::Enum(0)),
                TypeDefMembers::Class(_) | TypeDefMembers::StructLike(_) => {
                    let mut fields = Vec::new();
                    append_default_fields(typedef, schema, effective_args, &mut fields)?;
                    Ok(ContentValue::Composite(fields))
                }
            }
        }
    }
}

fn append_default_fields(
    typedef: &CerimalTypeDefinition,
    schema: &CerimalSchema,
    type_args: Option<&[CerimalTypeReference]>,
    fields: &mut Vec<(String, ContentNode)>,
) -> Result<(), String> {
    if let Some(base_info) = &typedef.base_info {
        if base_info.has_base {
            if let Some(base_ref) = &base_info.base_type_ref {
                let resolved_base = resolve_type_arg(base_ref, type_args);
                if let CerimalTypeReference::Named { guid, type_args: base_args } = resolved_base {
                    let base_idx = schema
                        .guid_map
                        .get(guid)
                        .copied()
                        .ok_or_else(|| "unknown base type guid".to_string())?;
                    let inherited_args = if !base_args.is_empty() {
                        Some(base_args.as_slice())
                    } else {
                        type_args
                    };
                    append_default_fields(
                        &schema.type_defs[base_idx],
                        schema,
                        inherited_args,
                        fields,
                    )?;
                }
            }
        }
    }

    match &typedef.members {
        TypeDefMembers::Class(members) => {
            for (member_idx, member) in members.iter().enumerate() {
                match member {
                    ClassMember::Field { name, type_ref } => {
                        fields.push((name.clone(), build_default_node(type_ref, schema, type_args)?));
                    }
                    ClassMember::List { element_type } => {
                        fields.push((
                            format!("$list{}", member_idx),
                            ContentNode::Concrete(Box::new(ConcreteNode {
                                type_ref: CerimalTypeReference::Array {
                                    rank: 1,
                                    element: Box::new(element_type.clone()),
                                },
                                value: ContentValue::Array {
                                    dims: vec![0],
                                    elements: Vec::new(),
                                },
                                node_start: 0,
                                value_start: 0,
                                value_end: 0,
                                register_in_backrefs: false,
                            })),
                        ));
                    }
                    ClassMember::Dict { .. } => {
                        fields.push((
                            format!("$dict{}", member_idx),
                            ContentNode::Concrete(Box::new(ConcreteNode {
                                type_ref: CerimalTypeReference::TypeArgument { index: 0 },
                                value: ContentValue::Composite(Vec::new()),
                                node_start: 0,
                                value_start: 0,
                                value_end: 0,
                                register_in_backrefs: false,
                            })),
                        ));
                    }
                }
            }
        }
        TypeDefMembers::StructLike(members) => {
            for member in members {
                fields.push((
                    member.name.clone(),
                    build_default_node(&member.type_ref, schema, type_args)?,
                ));
            }
        }
        TypeDefMembers::Enum(_) => return Err("cannot append fields for enum".to_string()),
    }

    Ok(())
}

fn default_primitive(wkt: WellKnownType) -> PrimitiveValue {
    match wkt {
        WellKnownType::Bool => PrimitiveValue::Bool(false),
        WellKnownType::SByte
        | WellKnownType::Short
        | WellKnownType::Int
        | WellKnownType::Long
        | WellKnownType::NInt => PrimitiveValue::I64(0),
        WellKnownType::Byte
        | WellKnownType::UShort
        | WellKnownType::UInt
        | WellKnownType::ULong
        | WellKnownType::NUInt
        | WellKnownType::Char
        | WellKnownType::AssetGuid => PrimitiveValue::U64(0),
        WellKnownType::Double | WellKnownType::Float => PrimitiveValue::F64(0.0),
        WellKnownType::Fp => PrimitiveValue::Fp(0.0),
        WellKnownType::Lfp => PrimitiveValue::Lfp(0.0),
        WellKnownType::String => PrimitiveValue::Str(String::new()),
        WellKnownType::Guid => PrimitiveValue::Guid([0; 16]),
        WellKnownType::Decimal => PrimitiveValue::Bytes(vec![0; 16]),
        WellKnownType::Unknown(_) => PrimitiveValue::U64(0),
    }
}

fn array_element_type(type_ref: &CerimalTypeReference) -> Result<&CerimalTypeReference, String> {
    match type_ref {
        CerimalTypeReference::Array { element, .. } => Ok(element.as_ref()),
        _ => Err("expected array type".to_string()),
    }
}

fn find_field_mut<'a>(
    fields: &'a mut [(String, ContentNode)],
    name: &str,
) -> Result<&'a mut ContentNode, String> {
    fields
        .iter_mut()
        .find(|(field_name, _)| field_name == name)
        .map(|(_, node)| node)
        .ok_or_else(|| format!("field '{}' not found", name))
}

fn find_dict_value_field_name(
    fields: &[(String, ContentNode)],
    wanted_key: &str,
    schema: &CerimalSchema,
) -> Option<String> {
    for (field_name, node) in fields {
        if !field_name.starts_with("$key") {
            continue;
        }

        if dict_key_matches(node, wanted_key, schema) {
            return Some(field_name.replacen("$key", "$val", 1));
        }
    }

    None
}

fn get_leaf_u64(node: &ContentNode, path: &str) -> Result<u64, String> {
    let node = find_node(node, path).ok_or_else(|| format!("field '{}' not found", path))?;
    let ContentNode::Concrete(node) = node.unwrap_poly() else {
        return Err(format!("field '{}' is not concrete", path));
    };

    match &node.value {
        ContentValue::Primitive(PrimitiveValue::U64(value)) => Ok(*value),
        ContentValue::Primitive(PrimitiveValue::I64(value)) if *value >= 0 => Ok(*value as u64),
        ContentValue::Enum(value) if *value >= 0 => Ok(*value as u64),
        _ => Err(format!("field '{}' is not an unsigned integer leaf", path)),
    }
}


fn dict_key_matches(node: &ContentNode, wanted_key: &str, schema: &CerimalSchema) -> bool {
    let ContentNode::Concrete(node) = node.unwrap_poly() else {
        return false;
    };

    match &node.value {
        ContentValue::Primitive(PrimitiveValue::Str(key)) => key == wanted_key,
        ContentValue::Enum(value) => {
            let CerimalTypeReference::Named { guid, .. } = &node.type_ref else {
                return false;
            };
            let Some(type_idx) = schema.guid_map.get(guid) else {
                return false;
            };
            let TypeDefMembers::Enum(members) = &schema.type_defs[*type_idx].members else {
                return false;
            };
            members
                .variants
                .iter()
                .any(|variant| variant.name == wanted_key && variant.value == *value)
        }
        _ => false,
    }
}

fn set_bool(node: &mut ContentNode, value: bool) -> Result<(), String> {
    let actual = unwrap_poly_mut(node);
    let ContentNode::Concrete(node) = actual else {
        return Err("expected concrete bool".to_string());
    };
    node.value = ContentValue::Primitive(PrimitiveValue::Bool(value));
    Ok(())
}

fn set_leaf_u64(node: &mut ContentNode, value: u64) -> Result<(), String> {
    let actual = unwrap_poly_mut(node);
    let ContentNode::Concrete(node) = actual else {
        return Err("expected concrete unsigned numeric node".to_string());
    };
    node.value = match &node.value {
        ContentValue::Primitive(PrimitiveValue::U64(_)) => {
            ContentValue::Primitive(PrimitiveValue::U64(value))
        }
        _ => return Err("unsupported unsigned numeric assignment target".to_string()),
    };
    Ok(())
}

fn set_enum(node: &mut ContentNode, value: i64) -> Result<(), String> {
    let actual = unwrap_poly_mut(node);
    let ContentNode::Concrete(node) = actual else {
        return Err("expected concrete enum".to_string());
    };
    node.value = ContentValue::Enum(value);
    Ok(())
}

fn set_leaf_numeric(node: &mut ContentNode, value: i64) -> Result<(), String> {
    let actual = unwrap_poly_mut(node);
    let ContentNode::Concrete(node) = actual else {
        return Err("expected concrete numeric node".to_string());
    };
    node.value = match &node.value {
        ContentValue::Primitive(PrimitiveValue::Bool(_)) => {
            return Err("cannot set bool from numeric helper".to_string())
        }
        ContentValue::Primitive(PrimitiveValue::I64(_)) => {
            ContentValue::Primitive(PrimitiveValue::I64(value))
        }
        ContentValue::Primitive(PrimitiveValue::U64(_)) => {
            if value < 0 {
                return Err("cannot assign negative value to unsigned target".to_string());
            }
            ContentValue::Primitive(PrimitiveValue::U64(value as u64))
        }
        ContentValue::Primitive(PrimitiveValue::F64(_)) => {
            ContentValue::Primitive(PrimitiveValue::F64(value as f64))
        }
        ContentValue::Primitive(PrimitiveValue::Fp(_)) => {
            ContentValue::Primitive(PrimitiveValue::Fp(value as f64))
        }
        ContentValue::Primitive(PrimitiveValue::Lfp(_)) => {
            ContentValue::Primitive(PrimitiveValue::Lfp(value as f64))
        }
        ContentValue::Primitive(PrimitiveValue::Str(_))
        | ContentValue::Primitive(PrimitiveValue::Guid(_))
        | ContentValue::Primitive(PrimitiveValue::Bytes(_))
        | ContentValue::Composite(_)
        | ContentValue::Array { .. }
        | ContentValue::Enum(_) => return Err("unsupported numeric assignment target".to_string()),
    };
    Ok(())
}

fn set_leaf_value(node: &mut ContentNode, value: &str) -> Result<(), String> {
    let actual = unwrap_poly_mut(node);
    let ContentNode::Concrete(node) = actual else {
        return Err("expected concrete leaf node".to_string());
    };
    node.value = match &node.value {
        ContentValue::Primitive(PrimitiveValue::Bool(_)) => {
            ContentValue::Primitive(PrimitiveValue::Bool(parse_bool(value)?))
        }
        ContentValue::Primitive(PrimitiveValue::I64(_)) => {
            ContentValue::Primitive(PrimitiveValue::I64(parse_i64(value)?))
        }
        ContentValue::Primitive(PrimitiveValue::U64(_)) => {
            ContentValue::Primitive(PrimitiveValue::U64(parse_u64(value)?))
        }
        ContentValue::Primitive(PrimitiveValue::F64(_)) => ContentValue::Primitive(
            PrimitiveValue::F64(value.parse::<f64>().map_err(|e| e.to_string())?),
        ),
        ContentValue::Primitive(PrimitiveValue::Fp(_)) => ContentValue::Primitive(
            PrimitiveValue::Fp(value.parse::<f64>().map_err(|e| e.to_string())?),
        ),
        ContentValue::Primitive(PrimitiveValue::Lfp(_)) => ContentValue::Primitive(
            PrimitiveValue::Lfp(value.parse::<f64>().map_err(|e| e.to_string())?),
        ),
        ContentValue::Enum(_) => ContentValue::Enum(parse_i64(value)?),
        _ => return Err("unsupported stat edit target".to_string()),
    };
    Ok(())
}

fn parse_i64(value: &str) -> Result<i64, String> {
    value.parse::<i64>().map_err(|e| e.to_string())
}

fn parse_u64(value: &str) -> Result<u64, String> {
    value.parse::<u64>().map_err(|e| e.to_string())
}

fn parse_bool(value: &str) -> Result<bool, String> {
    match value {
        "true" | "1" => Ok(true),
        "false" | "0" => Ok(false),
        _ => Err(format!("invalid bool '{}'", value)),
    }
}

fn sync_array_dims(dims: &mut Vec<usize>, len: usize) {
    if dims.is_empty() {
        dims.push(len);
    } else {
        dims[0] = len;
    }
}
