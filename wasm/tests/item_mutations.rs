use nrftw_wasm::document::{parse_all_docs, ParsedDoc};
use nrftw_wasm::mutations::{
    apply_changes, ApplyChangesRequest, ItemDelete, ItemDraft, StatEdit,
};
use nrftw_wasm::parser::content::{find_node, node_meta, ContentNode, ContentValue, PrimitiveValue};
use nrftw_wasm::parser::schema::{CerimalSchema, TypeDefMembers};
use nrftw_wasm::parser::types::CerimalTypeReference;

const CERIMAL: &str = "../examples/char_save_filled.cerimal";
const CHEFOOBAR: &str = "../examples/char_chefoobar.cerimal";
const FAILED_SAVE: &str = "../examples/failed_save.cerimal";
const DICT: &str = "../public/cerimal_zstd.dict";

fn load_fixture() -> (Vec<u8>, Vec<u8>) {
    let dict = std::fs::read(DICT).expect("read dict");
    let data = std::fs::read(CERIMAL).expect("read cerimal");
    (data, dict)
}

fn load_docs() -> Vec<ParsedDoc> {
    let (data, dict) = load_fixture();
    parse_all_docs(&data, &dict).expect("parse fixture")
}

fn load_chefoobar_fixture() -> (Vec<u8>, Vec<u8>) {
    let dict = std::fs::read(DICT).expect("read dict");
    let data = std::fs::read(CHEFOOBAR).expect("read cerimal");
    (data, dict)
}

fn load_chefoobar_docs() -> Vec<ParsedDoc> {
    let (data, dict) = load_chefoobar_fixture();
    parse_all_docs(&data, &dict).expect("parse chefoobar fixture")
}

fn load_failed_fixture() -> (Vec<u8>, Vec<u8>) {
    let dict = std::fs::read(DICT).expect("read dict");
    let data = std::fs::read(FAILED_SAVE).expect("read failed cerimal");
    (data, dict)
}

fn load_failed_docs() -> Vec<ParsedDoc> {
    let (data, dict) = load_failed_fixture();
    parse_all_docs(&data, &dict).expect("parse failed fixture")
}

fn doc1_value(docs: &[ParsedDoc], path: &str) -> String {
    let node = find_node(&docs[1].root, path).unwrap_or_else(|| panic!("missing node: {path}"));
    node_meta(node, &docs[1].schema)
        .value
        .unwrap_or_else(|| panic!("missing leaf value: {path}"))
}

fn doc1_child_count(docs: &[ParsedDoc], path: &str) -> usize {
    let node = find_node(&docs[1].root, path).unwrap_or_else(|| panic!("missing node: {path}"));
    node_meta(node, &docs[1].schema).child_count
}

fn doc1_enchantment_guids(docs: &[ParsedDoc], item_path: &str) -> Vec<String> {
    let list_path = format!("{item_path}.Item.Enchantment.value.Enchantment");
    let Some(node) = find_node(&docs[1].root, &list_path) else {
        return Vec::new();
    };
    let count = node_meta(node, &docs[1].schema).child_count;
    (0..count)
        .map(|index| doc1_value(docs, &format!("{item_path}.Item.Enchantment.value.Enchantment[{index}].Asset.Id")))
        .collect()
}

fn doc1_rune_guids(docs: &[ParsedDoc], item_path: &str) -> Vec<String> {
    let list_path = format!("{item_path}.Item.Weapon.value.RuneSlots");
    let Some(node) = find_node(&docs[1].root, &list_path) else {
        return Vec::new();
    };
    let count = node_meta(node, &docs[1].schema).child_count;
    (0..count)
        .map(|index| doc1_value(docs, &format!("{item_path}.Item.Weapon.value.RuneSlots[{index}].RuneDataId.Id")))
        .filter(|guid| guid != "0")
        .collect()
}

fn doc0_value(docs: &[ParsedDoc], path: &str) -> String {
    let node = find_node(&docs[0].root, path).unwrap_or_else(|| panic!("missing node: {path}"));
    node_meta(node, &docs[0].schema)
        .value
        .unwrap_or_else(|| panic!("missing leaf value: {path}"))
}

fn doc1_attribute_value(docs: &[ParsedDoc], key: &str) -> String {
    let dict = find_node(&docs[1].root, "Attributes.$dict0")
        .unwrap_or_else(|| panic!("missing attributes dict"));
    let dict = dict.unwrap_poly();
    let nrftw_wasm::parser::content::ContentNode::Concrete(node) = dict else {
        panic!("attributes dict is not concrete");
    };
    let nrftw_wasm::parser::content::ContentValue::Composite(fields) = &node.value else {
        panic!("attributes dict is not composite");
    };

    for (field_name, node) in fields {
        if !field_name.starts_with("$key") {
            continue;
        }

        if !dict_key_matches(node, key, &docs[1].schema) {
            continue;
        }

        let value_field = field_name.replacen("$key", "$val", 1);
        let value_node = fields
            .iter()
            .find(|(candidate, _)| candidate == &value_field)
            .map(|(_, node)| node)
            .unwrap_or_else(|| panic!("missing value node for {key}"));
        return node_meta(value_node, &docs[1].schema)
            .value
            .unwrap_or_else(|| panic!("missing attribute value for {key}"));
    }

    panic!("missing attribute key: {key}");
}

fn subtree_contains_backref(node: &ContentNode) -> bool {
    match node {
        ContentNode::Backref { .. } => true,
        ContentNode::Null(_) => false,
        ContentNode::Polymorphic { inner, .. } => subtree_contains_backref(inner),
        ContentNode::Concrete(node) => match &node.value {
            ContentValue::Primitive(_) | ContentValue::Enum(_) => false,
            ContentValue::Composite(fields) => {
                fields.iter().any(|(_, child)| subtree_contains_backref(child))
            }
            ContentValue::Array { elements, .. } => {
                elements.iter().any(subtree_contains_backref)
            }
        },
    }
}

fn collect_registered_backrefs_for_test(root: &ContentNode) -> Vec<ContentNode> {
    fn walk(node: &ContentNode, out: &mut Vec<ContentNode>) {
        match node {
            ContentNode::Null(_) | ContentNode::Backref { .. } => {}
            ContentNode::Polymorphic { inner, .. } => walk(inner, out),
            ContentNode::Concrete(node) => {
                match &node.value {
                    ContentValue::Primitive(_) | ContentValue::Enum(_) => {}
                    ContentValue::Composite(fields) => {
                        for (_, child) in fields {
                            walk(child, out);
                        }
                    }
                    ContentValue::Array { elements, .. } => {
                        for child in elements {
                            walk(child, out);
                        }
                    }
                }
                if node.register_in_backrefs {
                    out.push(ContentNode::Concrete(Box::new(node.as_ref().clone())));
                }
            }
        }
    }

    let mut out = Vec::new();
    walk(root, &mut out);
    out
}

fn collect_mismatched_enchantment_backrefs(docs: &[ParsedDoc]) -> Vec<String> {
    let backrefs = collect_registered_backrefs_for_test(&docs[1].root);
    let inventory = find_node(&docs[1].root, "Inventory").expect("inventory");
    let count = node_meta(inventory, &docs[1].schema).child_count;
    let mut mismatches = Vec::new();

    for index in 0..count {
        let item_path = format!("Inventory[{index}].Item.Enchantment.value");
        if find_node(&docs[1].root, &item_path).is_none() {
            continue;
        }

        for field in ["Enchantment", "Gems"] {
            let path = format!("{item_path}.{field}");
            let Some(node) = find_node(&docs[1].root, &path) else {
                continue;
            };
            let ContentNode::Backref {
                index: backref_index, ..
            } = node.unwrap_poly()
            else {
                continue;
            };
            let target = backrefs
                .get(*backref_index as usize)
                .unwrap_or_else(|| panic!("missing backref target {backref_index}"));
            let declared = node_meta(node, &docs[1].schema).type_str;
            let actual = node_meta(target, &docs[1].schema).type_str;
            if declared != actual {
                mismatches.push(format!(
                    "item={index} field={field} ref={backref_index} declared={declared} actual={actual}"
                ));
            }
        }
    }

    mismatches
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

fn unwrap_poly_mut(node: &mut ContentNode) -> &mut ContentNode {
    let mut current = node;
    loop {
        match current {
            ContentNode::Polymorphic { inner, .. } => current = inner.as_mut(),
            _ => return current,
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
    let (field_name, idx) = if let Some(bracket) = head.find('[') {
        let name = &head[..bracket];
        let idx = head[bracket + 1..].trim_end_matches(']').parse::<usize>().ok();
        (name, idx)
    } else {
        (head, None)
    };

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

fn clear_all_enchantment_arrays(doc: &mut ParsedDoc) {
    let inventory = find_node_mut(&mut doc.root, "Inventory").expect("inventory");
    let ContentNode::Concrete(node) = unwrap_poly_mut(inventory) else {
        panic!("inventory is not concrete");
    };
    let ContentValue::Array { elements, .. } = &mut node.value else {
        panic!("inventory is not an array");
    };

    for entry in elements.iter_mut() {
        let path = "Item.Enchantment.value.Enchantment";
        let Some(list) = find_node_mut(entry, path) else {
            continue;
        };
        let ContentNode::Concrete(node) = unwrap_poly_mut(list) else {
            continue;
        };
        let ContentValue::Array { dims, elements } = &mut node.value else {
            continue;
        };
        elements.clear();
        if dims.is_empty() {
            dims.push(0);
        } else {
            dims[0] = 0;
        }
    }
}

#[test]
fn edit_item_and_stat_roundtrip() {
    let mut docs = load_docs();
    let (_data, dict) = load_fixture();
    let original_contents_checksum = docs[1].header.checksum.to_string();

    let result = apply_changes(
        &mut docs,
        ApplyChangesRequest {
            stat_edits: vec![StatEdit {
                doc_idx: 0,
                path: "Level".to_string(),
                value: "20".to_string(),
            }],
            item_deletes: vec![],
            item_edits: vec![ItemDraft {
                doc_idx: 1,
                item_path: Some("Inventory[1]".to_string()),
                asset_guid: "1698349693959893818".to_string(),
                level: 9,
                rarity: 3,
                durability: 150,
                stack_count: 1,
                rune_guids: vec![],
                enchantment_guids: vec![
                    "3202028461714202202".to_string(),
                    "2824610576232940589".to_string(),
                ],
                enchantment_qualities: vec![],
                enchantment_exalt_stacks: vec![],
            }],
            item_creates: vec![],
        },
        &dict,
    )
    .expect("apply changes");

    let reparsed = parse_all_docs(&result, &dict).expect("reparse");

    assert_eq!(doc1_value(&reparsed, "Inventory[1].Item.ItemData.Id"), "1698349693959893818");
    assert_eq!(doc1_value(&reparsed, "Inventory[1].Item.Level"), "9");
    assert_eq!(doc1_value(&reparsed, "Inventory[1].Item.CreationLevel"), "9");
    assert_eq!(doc1_value(&reparsed, "Inventory[1].Item.Rarity"), "3");
    assert_eq!(doc1_value(&reparsed, "Inventory[1].Item.Durability.Durability"), "150");
    assert_eq!(doc1_value(&reparsed, "Inventory[1].Slot"), "4294967295");
    assert_eq!(
        doc1_value(&reparsed, "Inventory[1].Item.Enchantment.value.Enchantment[0].Asset.Id"),
        "3202028461714202202"
    );
    assert_eq!(
        doc1_value(&reparsed, "Inventory[1].Item.Enchantment.value.Enchantment[1].Asset.Id"),
        "2824610576232940589"
    );

    let doc0_level = find_node(&reparsed[0].root, "Level").expect("doc0 level");
    assert_eq!(
        node_meta(doc0_level, &reparsed[0].schema).value.as_deref(),
        Some("20")
    );
    assert_eq!(doc1_value(&reparsed, "Level"), "20");

    let updated_contents_checksum = reparsed[1].header.checksum.to_string();
    assert_eq!(
        doc0_value(&reparsed, "Common.ContentsChecksum"),
        updated_contents_checksum
    );
    assert_eq!(
        doc0_value(&reparsed, "Common.PendingContentsChecksum"),
        updated_contents_checksum
    );
    assert_eq!(
        doc0_value(&reparsed, "Common.PreviousContentsChecksum"),
        original_contents_checksum
    );
}

#[test]
fn metadata_stat_edits_sync_character_contents_fields() {
    let mut docs = load_docs();
    let (_data, dict) = load_fixture();
    let original_contents_checksum = docs[1].header.checksum.to_string();

    let result = apply_changes(
        &mut docs,
        ApplyChangesRequest {
            stat_edits: vec![
                StatEdit {
                    doc_idx: 0,
                    path: "Level".to_string(),
                    value: "22".to_string(),
                },
                StatEdit {
                    doc_idx: 0,
                    path: "XP".to_string(),
                    value: "12345".to_string(),
                },
                StatEdit {
                    doc_idx: 0,
                    path: "Gold".to_string(),
                    value: "777".to_string(),
                },
                StatEdit {
                    doc_idx: 0,
                    path: "Strength".to_string(),
                    value: "44".to_string(),
                },
                StatEdit {
                    doc_idx: 0,
                    path: "Load".to_string(),
                    value: "33".to_string(),
                },
                StatEdit {
                    doc_idx: 0,
                    path: "Health".to_string(),
                    value: "28".to_string(),
                },
            ],
            item_deletes: vec![],
            item_edits: vec![],
            item_creates: vec![],
        },
        &dict,
    )
    .expect("apply synced stat edits");

    let reparsed = parse_all_docs(&result, &dict).expect("reparse");

    assert_eq!(doc1_value(&reparsed, "Level"), "22");
    assert_eq!(doc1_value(&reparsed, "XP"), "12345");
    assert_eq!(doc1_value(&reparsed, "Gold"), "777");
    assert_eq!(doc1_attribute_value(&reparsed, "Strength"), "44");
    assert_eq!(doc1_attribute_value(&reparsed, "ItemLoad"), "33");
    assert_eq!(doc1_attribute_value(&reparsed, "Health"), "28");

    let updated_contents_checksum = reparsed[1].header.checksum.to_string();
    assert_eq!(
        doc0_value(&reparsed, "Common.ContentsChecksum"),
        updated_contents_checksum
    );
    assert_eq!(
        doc0_value(&reparsed, "Common.PendingContentsChecksum"),
        updated_contents_checksum
    );
    assert_eq!(
        doc0_value(&reparsed, "Common.PreviousContentsChecksum"),
        original_contents_checksum
    );
}

#[test]
fn create_item_appends_unequipped_inventory_entry() {
    let mut docs = load_docs();
    let (_data, dict) = load_fixture();
    let original_inventory_len = {
        let inv = find_node(&docs[1].root, "Inventory").expect("inventory");
        node_meta(inv, &docs[1].schema).child_count
    };

    let result = apply_changes(
        &mut docs,
        ApplyChangesRequest {
            stat_edits: vec![],
            item_deletes: vec![],
            item_edits: vec![],
            item_creates: vec![ItemDraft {
                doc_idx: 1,
                item_path: None,
                asset_guid: "1838235934543696561".to_string(),
                level: 6,
                rarity: 2,
                durability: 77,
                stack_count: 1,
                rune_guids: vec![],
                enchantment_guids: vec!["3202028461714202202".to_string()],
                enchantment_qualities: vec![],
                enchantment_exalt_stacks: vec![],
            }],
        },
        &dict,
    )
    .expect("apply create");

    let reparsed = parse_all_docs(&result, &dict).expect("reparse created");
    let new_index = original_inventory_len;
    let item_path = format!("Inventory[{new_index}]");

    assert_eq!(doc1_value(&reparsed, &format!("{item_path}.Item.ItemData.Id")), "1838235934543696561");
    assert_eq!(doc1_value(&reparsed, &format!("{item_path}.Item.Level")), "6");
    assert_eq!(doc1_value(&reparsed, &format!("{item_path}.Item.CreationLevel")), "6");
    assert_eq!(doc1_value(&reparsed, &format!("{item_path}.Item.Rarity")), "2");
    assert_eq!(doc1_value(&reparsed, &format!("{item_path}.Item.Durability.Durability")), "100");
    assert_eq!(doc1_value(&reparsed, &format!("{item_path}.Slot")), "4294967295");
    assert_eq!(
        doc1_value(
            &reparsed,
            &format!("{item_path}.Item.Enchantment.value.Enchantment[0].Asset.Id")
        ),
        "3202028461714202202"
    );
}

#[test]
fn created_item_materializes_template_backrefs() {
    let mut docs = load_chefoobar_docs();
    let (_, dict) = load_chefoobar_fixture();

    let original_inventory_len = {
        let inv = find_node(&docs[1].root, "Inventory").expect("inventory");
        node_meta(inv, &docs[1].schema).child_count
    };

    let result = apply_changes(
        &mut docs,
        ApplyChangesRequest {
            stat_edits: vec![],
            item_deletes: vec![],
            item_edits: vec![],
            item_creates: vec![ItemDraft {
                doc_idx: 1,
                item_path: None,
                asset_guid: "1838235934543696561".to_string(),
                level: 6,
                rarity: 2,
                durability: 77,
                stack_count: 1,
                rune_guids: vec![],
                enchantment_guids: vec!["3202028461714202202".to_string()],
                enchantment_qualities: vec![],
                enchantment_exalt_stacks: vec![],
            }],
        },
        &dict,
    )
    .expect("apply create");

    let reparsed = parse_all_docs(&result, &dict).expect("reparse created save");
    let created_path = format!("Inventory[{original_inventory_len}]");
    let created = find_node(&reparsed[1].root, &created_path).expect("created item");

    assert!(
        !subtree_contains_backref(created),
        "created item should not retain raw backrefs from the source template"
    );
}

#[test]
fn delete_item_removes_entry_and_reindexes_inventory() {
    let mut docs = load_docs();
    let (_data, dict) = load_fixture();
    let removed_path = "Inventory[1]";
    let shifted_guid = doc1_value(&docs, "Inventory[2].Item.ItemData.Id");
    let original_inventory_len = {
        let inv = find_node(&docs[1].root, "Inventory").expect("inventory");
        node_meta(inv, &docs[1].schema).child_count
    };

    let result = apply_changes(
        &mut docs,
        ApplyChangesRequest {
            stat_edits: vec![],
            item_deletes: vec![ItemDelete {
                doc_idx: 1,
                item_path: removed_path.to_string(),
            }],
            item_edits: vec![],
            item_creates: vec![],
        },
        &dict,
    )
    .expect("apply delete");

    let reparsed = parse_all_docs(&result, &dict).expect("reparse deleted");
    let inventory = find_node(&reparsed[1].root, "Inventory").expect("inventory");
    assert_eq!(
        node_meta(inventory, &reparsed[1].schema).child_count,
        original_inventory_len - 1
    );
    assert_eq!(
        doc1_value(&reparsed, "Inventory[1].Item.ItemData.Id"),
        shifted_guid
    );
}

#[test]
fn editing_level_preserves_slot_and_existing_enchantment_wrapper() {
    let mut docs = load_chefoobar_docs();
    let (_data, dict) = load_chefoobar_fixture();
    let item_path = "Inventory[8]";
    let original_asset_guid = doc1_value(&docs, &format!("{item_path}.Item.ItemData.Id"));
    let original_slot = doc1_value(&docs, &format!("{item_path}.Slot"));
    let original_has_value = doc1_value(&docs, &format!("{item_path}.Item.Enchantment.HasValue"));

    let result = apply_changes(
        &mut docs,
        ApplyChangesRequest {
            stat_edits: vec![],
            item_deletes: vec![],
            item_edits: vec![ItemDraft {
                doc_idx: 1,
                item_path: Some(item_path.to_string()),
                asset_guid: original_asset_guid,
                level: 15,
                rarity: 0,
                durability: 91,
                stack_count: 1,
                rune_guids: vec![],
                enchantment_guids: vec![],
                enchantment_qualities: vec![],
                enchantment_exalt_stacks: vec![],
            }],
            item_creates: vec![],
        },
        &dict,
    )
    .expect("apply level edit");

    let reparsed = parse_all_docs(&result, &dict).expect("reparse");

    assert_eq!(doc1_value(&reparsed, &format!("{item_path}.Item.Level")), "15");
    assert_eq!(doc1_value(&reparsed, &format!("{item_path}.Item.CreationLevel")), "15");
    assert_eq!(doc1_value(&reparsed, &format!("{item_path}.Slot")), original_slot);
    assert_eq!(
        doc1_value(&reparsed, &format!("{item_path}.Item.Enchantment.HasValue")),
        original_has_value
    );
}

#[test]
fn adding_enchantment_to_white_item_promotes_rarity_and_preserves_equipment_state() {
    let mut docs = load_chefoobar_docs();
    let (_data, dict) = load_chefoobar_fixture();
    let item_path = "Inventory[8]";
    let original_asset_guid = doc1_value(&docs, &format!("{item_path}.Item.ItemData.Id"));
    let original_slot = doc1_value(&docs, &format!("{item_path}.Slot"));
    let original_trait = doc1_value(
        &docs,
        &format!("{item_path}.Item.Enchantment.value.Trait.Asset.Id"),
    );
    let original_rune = doc1_value(
        &docs,
        &format!("{item_path}.Item.Weapon.value.RuneSlots[0].RuneDataId.Id"),
    );

    let result = apply_changes(
        &mut docs,
        ApplyChangesRequest {
            stat_edits: vec![],
            item_deletes: vec![],
            item_edits: vec![ItemDraft {
                doc_idx: 1,
                item_path: Some(item_path.to_string()),
                asset_guid: original_asset_guid,
                level: 10,
                rarity: 0,
                durability: 91,
                stack_count: 1,
                rune_guids: vec![],
                enchantment_guids: vec!["68574579388500304".to_string()],
                enchantment_qualities: vec![],
                enchantment_exalt_stacks: vec![],
            }],
            item_creates: vec![],
        },
        &dict,
    )
    .expect("apply enchantment edit");

    let reparsed = parse_all_docs(&result, &dict).expect("reparse");

    assert_eq!(doc1_value(&reparsed, &format!("{item_path}.Item.Rarity")), "1");
    assert_eq!(
        doc1_value(
            &reparsed,
            &format!("{item_path}.Item.Enchantment.value.Enchantment[0].Asset.Id")
        ),
        "68574579388500304"
    );
    assert_eq!(
        doc1_value(
            &reparsed,
            &format!("{item_path}.Item.Enchantment.value.Trait.Asset.Id")
        ),
        original_trait
    );
    assert_eq!(doc1_value(&reparsed, &format!("{item_path}.Slot")), original_slot);
    assert_eq!(
        doc1_value(
            &reparsed,
            &format!("{item_path}.Item.Weapon.value.RuneSlots[0].RuneDataId.Id")
        ),
        original_rune
    );
}

#[test]
fn removing_all_enchantments_clears_the_enchantment_array() {
    let mut docs = load_docs();
    let (_data, dict) = load_fixture();
    let item_path = "Inventory[1]";
    let original_asset_guid = doc1_value(&docs, &format!("{item_path}.Item.ItemData.Id"));

    let result = apply_changes(
        &mut docs,
        ApplyChangesRequest {
            stat_edits: vec![],
            item_deletes: vec![],
            item_edits: vec![ItemDraft {
                doc_idx: 1,
                item_path: Some(item_path.to_string()),
                asset_guid: original_asset_guid,
                level: 9,
                rarity: 3,
                durability: 150,
                stack_count: 1,
                rune_guids: vec![],
                enchantment_guids: vec![],
                enchantment_qualities: vec![],
                enchantment_exalt_stacks: vec![],
            }],
            item_creates: vec![],
        },
        &dict,
    )
    .expect("apply enchantment removal edit");

    let reparsed = parse_all_docs(&result, &dict).expect("reparse");

    assert_eq!(doc1_value(&reparsed, &format!("{item_path}.Item.Enchantment.HasValue")), "false");
    assert_eq!(
        doc1_child_count(&reparsed, &format!("{item_path}.Item.Enchantment.value.Enchantment")),
        0
    );
}

#[test]
fn create_item_with_four_enchantments_promotes_to_purple() {
    let mut docs = load_docs();
    let (_data, dict) = load_fixture();
    let original_inventory_len = {
        let inv = find_node(&docs[1].root, "Inventory").expect("inventory");
        node_meta(inv, &docs[1].schema).child_count
    };

    let result = apply_changes(
        &mut docs,
        ApplyChangesRequest {
            stat_edits: vec![],
            item_deletes: vec![],
            item_edits: vec![],
            item_creates: vec![ItemDraft {
                doc_idx: 1,
                item_path: None,
                asset_guid: "1838235934543696561".to_string(),
                level: 6,
                rarity: 0,
                durability: 77,
                stack_count: 1,
                rune_guids: vec![],
                enchantment_guids: vec![
                    "3202028461714202202".to_string(),
                    "2824610576232940589".to_string(),
                    "3865599436897390395".to_string(),
                    "1171821572034496964".to_string(),
                ],
                enchantment_qualities: vec![],
                enchantment_exalt_stacks: vec![],
            }],
        },
        &dict,
    )
    .expect("apply create");

    let reparsed = parse_all_docs(&result, &dict).expect("reparse created");
    let item_path = format!("Inventory[{original_inventory_len}]");

    assert_eq!(doc1_value(&reparsed, &format!("{item_path}.Item.Rarity")), "2");
    assert_eq!(doc1_value(&reparsed, &format!("{item_path}.Item.Durability.Durability")), "100");
}

#[test]
fn create_item_with_enchantments_does_not_require_existing_enchanted_items() {
    let mut docs = load_docs();
    clear_all_enchantment_arrays(&mut docs[1]);
    let (_data, dict) = load_fixture();
    let original_inventory_len = {
        let inv = find_node(&docs[1].root, "Inventory").expect("inventory");
        node_meta(inv, &docs[1].schema).child_count
    };

    let result = apply_changes(
        &mut docs,
        ApplyChangesRequest {
            stat_edits: vec![],
            item_deletes: vec![],
            item_edits: vec![],
            item_creates: vec![ItemDraft {
                doc_idx: 1,
                item_path: None,
                asset_guid: "1838235934543696561".to_string(),
                level: 6,
                rarity: 2,
                durability: 77,
                stack_count: 1,
                rune_guids: vec![],
                enchantment_guids: vec!["3202028461714202202".to_string()],
                enchantment_qualities: vec![],
                enchantment_exalt_stacks: vec![],
            }],
        },
        &dict,
    )
    .expect("apply create without templates");

    let reparsed = parse_all_docs(&result, &dict).expect("reparse created");
    let item_path = format!("Inventory[{original_inventory_len}]");

    assert_eq!(
        doc1_value(
            &reparsed,
            &format!("{item_path}.Item.Enchantment.value.Enchantment[0].Asset.Id")
        ),
        "3202028461714202202"
    );
}

#[test]
fn adding_enchantment_does_not_require_existing_enchantment_entries_anywhere() {
    let mut docs = load_chefoobar_docs();
    clear_all_enchantment_arrays(&mut docs[1]);
    let (_data, dict) = load_chefoobar_fixture();
    let item_path = "Inventory[8]";
    let original_asset_guid = doc1_value(&docs, &format!("{item_path}.Item.ItemData.Id"));

    let result = apply_changes(
        &mut docs,
        ApplyChangesRequest {
            stat_edits: vec![],
            item_deletes: vec![],
            item_edits: vec![ItemDraft {
                doc_idx: 1,
                item_path: Some(item_path.to_string()),
                asset_guid: original_asset_guid,
                level: 10,
                rarity: 0,
                durability: 91,
                stack_count: 1,
                rune_guids: vec![],
                enchantment_guids: vec!["68574579388500304".to_string()],
                enchantment_qualities: vec![],
                enchantment_exalt_stacks: vec![],
            }],
            item_creates: vec![],
        },
        &dict,
    )
    .expect("apply enchantment edit without templates");

    let reparsed = parse_all_docs(&result, &dict).expect("reparse");

    assert_eq!(
        doc1_value(
            &reparsed,
            &format!("{item_path}.Item.Enchantment.value.Enchantment[0].Asset.Id")
        ),
        "68574579388500304"
    );
}

#[test]
fn create_stackable_item_persists_stack_count() {
    let mut docs = load_docs();
    let (_data, dict) = load_fixture();
    let original_inventory_len = {
        let inv = find_node(&docs[1].root, "Inventory").expect("inventory");
        node_meta(inv, &docs[1].schema).child_count
    };

    let result = apply_changes(
        &mut docs,
        ApplyChangesRequest {
            stat_edits: vec![],
            item_deletes: vec![],
            item_edits: vec![],
            item_creates: vec![ItemDraft {
                doc_idx: 1,
                item_path: None,
                asset_guid: "334358031176021315".to_string(),
                level: 1,
                rarity: 0,
                durability: 0,
                stack_count: 12,
                rune_guids: vec![],
                enchantment_guids: vec![],
                enchantment_qualities: vec![],
                enchantment_exalt_stacks: vec![],
            }],
        },
        &dict,
    )
    .expect("apply stacked create");

    let reparsed = parse_all_docs(&result, &dict).expect("reparse stacked create");
    let item_path = format!("Inventory[{original_inventory_len}]");

    assert_eq!(doc1_value(&reparsed, &format!("{item_path}.Item.ItemData.Id")), "334358031176021315");
    assert_eq!(doc1_value(&reparsed, &format!("{item_path}.Item.Stack.Count")), "12");
}

#[test]
fn create_weapon_with_runes_from_slotless_template() {
    // Regression: creating a weapon with rune guids when the inventory template item
    // has 0 rune slots should succeed by expanding the slot array, not error.
    let mut docs = load_docs();
    let (_data, dict) = load_fixture();
    // Use a template item that has no rune slots (Inventory[0] in char_save_filled).
    // Pick the source asset guid from a weapon already in inventory.
    let source_asset_guid = doc1_value(&docs, "Inventory[0].Item.ItemData.Id");
    let template_runes = doc1_rune_guids(&docs, "Inventory[0]");
    assert!(template_runes.is_empty(), "Inventory[0] should have no rune slots to test expansion");

    let original_inventory_len = {
        let inv = find_node(&docs[1].root, "Inventory").expect("inventory");
        node_meta(inv, &docs[1].schema).child_count
    };

    // Fake rune guids (non-zero) to assign to the new item.
    let rune_guids = vec!["111111111111111111".to_string(), "222222222222222222".to_string()];

    let result = apply_changes(
        &mut docs,
        ApplyChangesRequest {
            stat_edits: vec![],
            item_deletes: vec![],
            item_edits: vec![],
            item_creates: vec![ItemDraft {
                doc_idx: 1,
                item_path: None,
                asset_guid: source_asset_guid,
                level: 10,
                rarity: 3,
                durability: 100,
                stack_count: 1,
                rune_guids: rune_guids.clone(),
                enchantment_guids: vec![],
                enchantment_qualities: vec![],
                enchantment_exalt_stacks: vec![],
            }],
        },
        &dict,
    )
    .expect("should succeed even when template has no rune slots");

    let reparsed = parse_all_docs(&result, &dict).expect("reparse created");
    let item_path = format!("Inventory[{original_inventory_len}]");
    assert_eq!(doc1_rune_guids(&reparsed, &item_path), rune_guids);
}

#[test]
fn create_weapon_with_runes_persists_rune_slots() {
    let mut docs = load_chefoobar_docs();
    let (_data, dict) = load_chefoobar_fixture();
    let source_item_path = "Inventory[8]";
    let original_inventory_len = {
        let inv = find_node(&docs[1].root, "Inventory").expect("inventory");
        node_meta(inv, &docs[1].schema).child_count
    };
    let source_asset_guid = doc1_value(&docs, &format!("{source_item_path}.Item.ItemData.Id"));
    let source_runes = doc1_rune_guids(&docs, source_item_path);
    assert!(!source_runes.is_empty(), "source weapon should have runes in fixture");

    let result = apply_changes(
        &mut docs,
        ApplyChangesRequest {
            stat_edits: vec![],
            item_deletes: vec![],
            item_edits: vec![],
            item_creates: vec![ItemDraft {
                doc_idx: 1,
                item_path: None,
                asset_guid: source_asset_guid,
                level: 10,
                rarity: 3,
                durability: 100,
                stack_count: 1,
                rune_guids: source_runes.clone(),
                enchantment_guids: vec![],
                enchantment_qualities: vec![],
                enchantment_exalt_stacks: vec![],
            }],
        },
        &dict,
    )
    .expect("apply create with runes");

    let reparsed = parse_all_docs(&result, &dict).expect("reparse created");
    let item_path = format!("Inventory[{original_inventory_len}]");
    assert_eq!(doc1_rune_guids(&reparsed, &item_path), source_runes);
}

#[test]
fn mixed_create_edit_delete_batch_roundtrips_with_rebased_inventory_paths() {
    let mut docs = load_docs();
    let (_data, dict) = load_fixture();
    let original_inventory_len = {
        let inv = find_node(&docs[1].root, "Inventory").expect("inventory");
        node_meta(inv, &docs[1].schema).child_count
    };

    let original_zero_guid = doc1_value(&docs, "Inventory[0].Item.ItemData.Id");
    let original_one_guid = doc1_value(&docs, "Inventory[1].Item.ItemData.Id");
    let original_two_guid = doc1_value(&docs, "Inventory[2].Item.ItemData.Id");
    let original_forty_guid = doc1_value(&docs, "Inventory[40].Item.ItemData.Id");
    let original_forty_two_guid = doc1_value(&docs, "Inventory[42].Item.ItemData.Id");

    let edited_first_level = doc1_value(&docs, "Inventory[2].Item.Level")
        .parse::<i64>()
        .expect("parse level")
        + 5;
    let edited_first_rarity = doc1_value(&docs, "Inventory[2].Item.Rarity")
        .parse::<i64>()
        .expect("parse rarity");
    let edited_first_durability = doc1_value(&docs, "Inventory[2].Item.Durability.Durability")
        .parse::<i64>()
        .expect("parse durability");
    let edited_first_stack = doc1_value(&docs, "Inventory[2].Item.Stack.Count")
        .parse::<i64>()
        .expect("parse stack");
    let edited_first_enchantments = doc1_enchantment_guids(&docs, "Inventory[2]");

    let edited_second_level = doc1_value(&docs, "Inventory[52].Item.Level")
        .parse::<i64>()
        .expect("parse level")
        + 2;
    let edited_second_guid = doc1_value(&docs, "Inventory[52].Item.ItemData.Id");
    let edited_second_rarity = doc1_value(&docs, "Inventory[52].Item.Rarity")
        .parse::<i64>()
        .expect("parse rarity");
    let edited_second_durability = doc1_value(&docs, "Inventory[52].Item.Durability.Durability")
        .parse::<i64>()
        .expect("parse durability");
    let edited_second_stack = doc1_value(&docs, "Inventory[52].Item.Stack.Count")
        .parse::<i64>()
        .expect("parse stack");
    let edited_second_enchantments = doc1_enchantment_guids(&docs, "Inventory[52]");

    let result = apply_changes(
        &mut docs,
        ApplyChangesRequest {
            stat_edits: vec![],
            item_deletes: vec![
                ItemDelete {
                    doc_idx: 1,
                    item_path: format!("Inventory[{}]", original_inventory_len - 1),
                },
                ItemDelete {
                    doc_idx: 1,
                    item_path: "Inventory[0]".to_string(),
                },
                ItemDelete {
                    doc_idx: 1,
                    item_path: "Inventory[40]".to_string(),
                },
            ],
            item_edits: vec![
                ItemDraft {
                    doc_idx: 1,
                    item_path: Some("Inventory[1]".to_string()),
                    asset_guid: original_two_guid.clone(),
                    level: edited_first_level,
                    rarity: edited_first_rarity,
                    durability: edited_first_durability,
                    stack_count: edited_first_stack,
                    rune_guids: vec![],
                    enchantment_guids: edited_first_enchantments,
                    enchantment_qualities: vec![],
                    enchantment_exalt_stacks: vec![],
                },
                ItemDraft {
                    doc_idx: 1,
                    item_path: Some("Inventory[50]".to_string()),
                    asset_guid: edited_second_guid.clone(),
                    level: edited_second_level,
                    rarity: edited_second_rarity,
                    durability: edited_second_durability,
                    stack_count: edited_second_stack,
                    rune_guids: vec![],
                    enchantment_guids: edited_second_enchantments,
                    enchantment_qualities: vec![],
                    enchantment_exalt_stacks: vec![],
                },
            ],
            item_creates: vec![
                ItemDraft {
                    doc_idx: 1,
                    item_path: None,
                    asset_guid: "1838235934543696561".to_string(),
                    level: 6,
                    rarity: 2,
                    durability: 77,
                    stack_count: 1,
                    rune_guids: vec![],
                    enchantment_guids: vec!["3202028461714202202".to_string()],
                    enchantment_qualities: vec![],
                    enchantment_exalt_stacks: vec![],
                },
                ItemDraft {
                    doc_idx: 1,
                    item_path: None,
                    asset_guid: "334358031176021315".to_string(),
                    level: 1,
                    rarity: 0,
                    durability: 0,
                    stack_count: 12,
                    rune_guids: vec![],
                    enchantment_guids: vec![],
                    enchantment_qualities: vec![],
                    enchantment_exalt_stacks: vec![],
                },
                ItemDraft {
                    doc_idx: 1,
                    item_path: None,
                    asset_guid: "1698349693959893818".to_string(),
                    level: 9,
                    rarity: 1,
                    durability: 88,
                    stack_count: 1,
                    rune_guids: vec![],
                    enchantment_guids: vec!["3202028461714202202".to_string()],
                    enchantment_qualities: vec![],
                    enchantment_exalt_stacks: vec![],
                },
            ],
        },
        &dict,
    )
    .expect("apply mixed batch");

    let reparsed = parse_all_docs(&result, &dict).expect("reparse mixed batch");
    let inventory = find_node(&reparsed[1].root, "Inventory").expect("inventory");
    assert_eq!(
        node_meta(inventory, &reparsed[1].schema).child_count,
        original_inventory_len
    );

    assert_ne!(doc1_value(&reparsed, "Inventory[0].Item.ItemData.Id"), original_zero_guid);
    assert_eq!(doc1_value(&reparsed, "Inventory[0].Item.ItemData.Id"), original_one_guid);
    assert_eq!(doc1_value(&reparsed, "Inventory[1].Item.ItemData.Id"), original_two_guid);
    assert_eq!(doc1_value(&reparsed, "Inventory[1].Item.Level"), edited_first_level.to_string());
    assert_eq!(doc1_value(&reparsed, "Inventory[39].Item.ItemData.Id"), original_forty_guid);
    assert_eq!(doc1_value(&reparsed, "Inventory[40].Item.ItemData.Id"), original_forty_two_guid);
    assert_eq!(doc1_value(&reparsed, "Inventory[50].Item.ItemData.Id"), edited_second_guid);
    assert_eq!(doc1_value(&reparsed, "Inventory[50].Item.Level"), edited_second_level.to_string());

    let created_start = original_inventory_len - 3;
    assert_eq!(
        doc1_value(&reparsed, &format!("Inventory[{created_start}].Item.ItemData.Id")),
        "1838235934543696561"
    );
    assert_eq!(
        doc1_value(
            &reparsed,
            &format!("Inventory[{}].Item.Stack.Count", created_start + 1),
        ),
        "12"
    );
    assert_eq!(
        doc1_value(
            &reparsed,
            &format!("Inventory[{}].Item.Level", created_start + 2),
        ),
        "9"
    );
}

#[test]
fn failed_save_contents_edit_rewrites_mismatched_enchantment_backrefs() {
    let mut docs = load_failed_docs();
    let (_, dict) = load_failed_fixture();

    let before = collect_mismatched_enchantment_backrefs(&docs);
    assert!(
        !before.is_empty(),
        "failed save fixture should start with mismatched enchantment/gem backrefs"
    );

    let item_draft = ItemDraft {
        doc_idx: 1,
        item_path: Some("Inventory[11]".to_string()),
        asset_guid: doc1_value(&docs, "Inventory[11].Item.ItemData.Id"),
        level: doc1_value(&docs, "Inventory[11].Item.Level")
            .parse::<i64>()
            .expect("parse level"),
        rarity: doc1_value(&docs, "Inventory[11].Item.Rarity")
            .parse::<i64>()
            .expect("parse rarity"),
        durability: doc1_value(&docs, "Inventory[11].Item.Durability.Durability")
            .parse::<i64>()
            .expect("parse durability"),
        stack_count: doc1_value(&docs, "Inventory[11].Item.Stack.Count")
            .parse::<i64>()
            .expect("parse stack"),
        rune_guids: doc1_rune_guids(&docs, "Inventory[11]"),
        enchantment_guids: doc1_enchantment_guids(&docs, "Inventory[11]"),
        enchantment_qualities: vec![],
        enchantment_exalt_stacks: vec![],
    };

    let result = apply_changes(
        &mut docs,
        ApplyChangesRequest {
            stat_edits: vec![],
            item_deletes: vec![],
            item_edits: vec![item_draft],
            item_creates: vec![],
        },
        &dict,
    )
    .expect("rewrite failed save");

    let reparsed = parse_all_docs(&result, &dict).expect("reparse rewritten failed save");
    let after = collect_mismatched_enchantment_backrefs(&reparsed);
    assert_eq!(after, Vec::<String>::new(), "mismatches after rewrite: {after:?}");
}
