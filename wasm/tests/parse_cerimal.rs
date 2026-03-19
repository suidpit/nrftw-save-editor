use nrftw_wasm::document::{parse_all_docs, ParsedDoc};
use nrftw_wasm::parser::{
    content::{children_of, find_node, node_meta},
    schema::fmt_type_ref,
};

const CERIMAL: &str = "../examples/char_save_filled.cerimal";
const DICT: &str = "../public/cerimal_zstd.dict";

fn load_docs() -> Vec<ParsedDoc> {
    let dict = std::fs::read(DICT).expect("read dict");
    let data = std::fs::read(CERIMAL).expect("read cerimal");
    parse_all_docs(&data, &dict).expect("parse_all_docs failed")
}

#[test]
fn test_invalid_input_returns_error() {
    let dict = std::fs::read(DICT).expect("read dict");
    match parse_all_docs(b"not a cerimal file", &dict) {
        Ok(docs) => panic!("invalid input should fail, got {} docs", docs.len()),
        Err(err) => assert!(
            err.contains("no CERIMAL documents found"),
            "unexpected error: {}",
            err
        ),
    }
}

fn doc_fields(doc: &ParsedDoc) -> Vec<(String, String, String)> {
    let kids = children_of(&doc.root, "", &doc.schema);
    kids.into_iter().map(|c| {
        let val = c.meta.value.unwrap_or_default();
        (c.key, c.meta.type_str, val)
    }).collect()
}

#[test]
fn test_root_type() {
    let docs = load_docs();
    assert!(!docs.is_empty(), "no documents parsed");
    let root_type = fmt_type_ref(&docs[0].schema.root_type_ref, &docs[0].schema);
    assert_eq!(root_type, "Quantum.CharacterMetadata", "root type mismatch: got {}", root_type);
    println!("✓ Doc 0 root type: {}", root_type);
}

#[test]
fn test_root_fields_exist() {
    let docs = load_docs();
    let fields = doc_fields(&docs[0]);

    println!("Root fields ({}):", fields.len());
    for (k, t, v) in &fields {
        println!("  {}: {} = {}", k, t, v);
    }

    let field_names: Vec<&str> = fields.iter().map(|(k, _, _)| k.as_str()).collect();
    assert!(!field_names.is_empty(), "no root fields found");

    let expected = ["Common", "Hardcore", "Level", "XP", "Health", "Gold"];
    for name in &expected {
        assert!(
            field_names.contains(name),
            "expected field '{}' in root, got: {:?}",
            name,
            &field_names
        );
    }
    println!("✓ All expected fields present");
}

#[test]
fn test_numeric_fields() {
    let docs = load_docs();
    let fields = doc_fields(&docs[0]);

    for name in &["Level", "Health", "Gold", "Strength", "Dexterity"] {
        if let Some((_, ty, val)) = fields.iter().find(|(k, _, _)| k == name) {
            let parsed: Result<i64, _> = val.parse();
            assert!(parsed.is_ok(), "field {} (type={}) value {:?} is not an integer", name, ty, val);
            println!("  {} ({}) = {}", name, ty, val);
        }
    }
    println!("✓ Numeric fields parse correctly");
}

#[test]
fn test_exact_values_doc0() {
    let docs = load_docs();
    let fields = doc_fields(&docs[0]);

    let find = |name: &str| -> &str {
        &fields.iter().find(|(k, _, _)| k == name).unwrap().2
    };

    assert_eq!(find("Level"), "15");
    assert_eq!(find("Health"), "18");
    assert_eq!(find("Gold"), "7394");
    assert_eq!(find("Strength"), "10");
    assert_eq!(find("Dexterity"), "26");
    assert_eq!(find("Intelligence"), "10");
    assert_eq!(find("Faith"), "10");
    assert_eq!(find("Focus"), "13");
    assert_eq!(find("Load"), "17");
    assert_eq!(find("Stamina"), "18");
    assert_eq!(find("Hardcore"), "false");
    assert_eq!(find("TutorialSkippedOrCompleted"), "true");

    let xp: f64 = find("XP").parse().unwrap();
    assert!((xp - 938.0).abs() < 0.01, "XP mismatch: {}", xp);

    println!("✓ All Doc 0 values match Python reference");
}

#[test]
fn test_exact_values_doc1() {
    let docs = load_docs();
    assert!(docs.len() >= 2);
    let fields = doc_fields(&docs[1]);

    let find = |name: &str| -> &str {
        &fields.iter().find(|(k, _, _)| k == name)
            .unwrap_or_else(|| panic!("field '{}' not found", name)).2
    };

    assert_eq!(find("CharacterName"), "\"tiren\"");
    assert_eq!(find("CharacterCreationTimeUtcTicks"), "639050647366196669");
    assert_eq!(find("Level"), "15");
    assert_eq!(find("Gold"), "7394");
    assert_eq!(find("XP"), "938");
    assert_eq!(find("CurrentRightHandItem"), "1");
    assert_eq!(find("HasLastRestedAtBonfirePosition"), "false");
    assert_eq!(find("InitialRealmSetupHandled"), "true");

    let hp: f64 = find("HP").parse().unwrap();
    assert!((hp - 171.999).abs() < 0.01, "HP mismatch: {}", hp);

    // Check inventory array length matches Python (108 items)
    let inv_node = find_node(&docs[1].root, "Inventory").expect("Inventory not found");
    let inv_children = children_of(inv_node, "Inventory", &docs[1].schema);
    assert_eq!(inv_children.len(), 108, "Inventory item count mismatch: got {}", inv_children.len());
    println!("  Inventory: {} items ✓", inv_children.len());

    println!("✓ All Doc 1 values match Python reference");
}

#[test]
fn test_doc1_character_contents() {
    let docs = load_docs();
    assert!(docs.len() >= 2, "expected at least 2 documents, got {}", docs.len());

    let root_type = fmt_type_ref(&docs[1].schema.root_type_ref, &docs[1].schema);
    assert_eq!(root_type, "Quantum.CharacterContents", "doc 1 root type mismatch: got {}", root_type);

    let fields = doc_fields(&docs[1]);
    println!("Doc 1 root fields ({}):", fields.len());
    for (k, t, v) in &fields {
        let display = if v.len() > 60 { format!("{}…", &v[..60]) } else { v.clone() };
        println!("  {}: {} = {}", k, t, display);
    }

    let field_names: Vec<&str> = fields.iter().map(|(k, _, _)| k.as_str()).collect();
    let expected = ["Identifier", "CharacterName", "Inventory", "Gold", "Level", "HP"];
    for name in &expected {
        assert!(
            field_names.contains(name),
            "expected field '{}' in doc 1 root, got: {:?}",
            name,
            &field_names
        );
    }
    assert!(!fields.is_empty(), "doc 1 has no root fields — content parse likely failed");
    println!("✓ Doc 1 parsed with {} fields", fields.len());
}

#[test]
fn test_exalt_stacks_in_snapshot() {
    use nrftw_wasm::inventory_snapshot;

    let dict = std::fs::read(DICT).expect("read dict");
    let data = std::fs::read("../examples/char_exalted.cerimal").expect("read exalted save");
    let docs = parse_all_docs(&data, &dict).expect("parse exalted save");
    let snapshot = inventory_snapshot(&docs[1]).expect("snapshot");

    let exalted_items: Vec<_> = snapshot.iter()
        .filter(|i| i.enchantment_exalt_stacks.iter().any(|&s| s > 0))
        .collect();
    assert!(!exalted_items.is_empty(), "expected exalted items in test save");

    for item in &exalted_items {
        let total: i64 = item.enchantment_exalt_stacks.iter().sum();
        assert!(total <= 4, "item {} has total exalt {} > 4", item.item_path, total);
        println!("  exalted item {} — stacks: {:?}", item.item_path, item.enchantment_exalt_stacks);
    }

    println!("✓ {} exalted items found, all within cap", exalted_items.len());
}

const CATALOG_DB: &str = "../public/catalog.db";

fn parse_doc0() -> ParsedDoc {
    load_docs().remove(0)
}

#[test]
fn test_equipment_guids_in_catalog() {
    let doc = parse_doc0();

    let db_path = std::path::Path::new(CATALOG_DB);
    if !db_path.exists() {
        eprintln!("Skipping catalog test: {} not found", CATALOG_DB);
        return;
    }

    let expected: &[(&str, u64, &str)] = &[
        ("RightHand", 1023863541289577995, "Laquered Bow"),
        ("LeftHand",  716007893309524987,  "Wooden Shield"),
        ("Helmet",    1838235934543696561, "Wildling Helmet"),
        ("Body",      1797774859941529097, "Nith Leviathan Cloak"),
        ("Gloves",    3540398148196568207, "Bolein Rebels Fists"),
        ("Pants",     1698349693959893818, "Farmers Cords"),
    ];

    let mut extracted_guids: Vec<(&str, u64)> = Vec::new();
    for (slot, expected_guid, _) in expected {
        let id_path = format!("{}.Id", slot);
        let id_node = find_node(&doc.root, &id_path)
            .unwrap_or_else(|| panic!("node '{}' not found", id_path));
        let meta = node_meta(id_node, &doc.schema);
        assert_eq!(meta.type_str, "ASSETGUID", "{} type mismatch: {}", slot, meta.type_str);
        let guid_str = meta.guid.as_ref()
            .unwrap_or_else(|| panic!("{}.Id has no guid in meta", slot));
        let guid_val: u64 = guid_str.parse()
            .unwrap_or_else(|_| panic!("{}.Id guid '{}' is not a u64", slot, guid_str));
        assert_eq!(guid_val, *expected_guid, "{} ASSETGUID mismatch", slot);
        extracted_guids.push((slot, guid_val));
        println!("  {} ASSETGUID = {}", slot, guid_val);
    }

    for (slot, guid, expected_name) in expected {
        let output = std::process::Command::new("sqlite3")
            .arg(CATALOG_DB)
            .arg(format!(
                "SELECT display_name FROM assets WHERE asset_guid = {};",
                guid
            ))
            .output()
            .expect("failed to run sqlite3 CLI");
        let display_name = String::from_utf8_lossy(&output.stdout).trim().to_string();
        assert_eq!(
            &display_name, expected_name,
            "{} catalog lookup: got '{}', expected '{}'",
            slot, display_name, expected_name
        );
        println!("  {} ({}) => {} ✓", slot, guid, display_name);
    }

    println!("✓ All equipment GUIDs found in catalog with correct names");
}

#[test]
fn test_snapshot_enchantment_qualities_and_exalt_stacks() {
    use nrftw_wasm::inventory_snapshot;

    let docs = load_docs();
    let snapshot = inventory_snapshot(&docs[1]).expect("inventory_snapshot failed");

    // All three parallel arrays must have identical lengths on every item
    for item in &snapshot {
        assert_eq!(
            item.enchantment_guids.len(),
            item.enchantment_qualities.len(),
            "enchantment_qualities length mismatch at {}",
            item.item_path
        );
        assert_eq!(
            item.enchantment_guids.len(),
            item.enchantment_exalt_stacks.len(),
            "enchantment_exalt_stacks length mismatch at {}",
            item.item_path
        );
        // Every quality string must parse as a valid u64
        for q in &item.enchantment_qualities {
            q.parse::<u64>().unwrap_or_else(|_| {
                panic!("quality '{}' at {} is not a valid u64", q, item.item_path)
            });
        }
    }

    let enchanted: Vec<_> = snapshot
        .iter()
        .filter(|i| !i.enchantment_guids.is_empty())
        .collect();
    assert!(!enchanted.is_empty(), "expected at least one enchanted item in test save");

    // Spot-check: find the item with the known enchantment "Movement Speed Increased After Kill"
    // (guid 3202028461714202202, raw Quality 5534276545726513152 → q ≈ 0.30 → rolled = 9%)
    let target_guid = "3202028461714202202";
    let expected_quality = "5534276545726513152";

    if let Some(item) = snapshot.iter().find(|i| i.enchantment_guids.iter().any(|g| g == target_guid)) {
        let idx = item.enchantment_guids.iter().position(|g| g == target_guid).unwrap();
        assert_eq!(
            item.enchantment_qualities[idx], expected_quality,
            "quality mismatch for {} at {}",
            target_guid,
            item.item_path
        );
        assert_eq!(
            item.enchantment_exalt_stacks[idx], 0,
            "expected ExaltStacks=0 for {} at {}",
            target_guid,
            item.item_path
        );
        println!("  ✓ {} quality={} at {}", target_guid, expected_quality, item.item_path);
    } else {
        println!("  Note: guid {} not in inventory (may be in equipped slot)", target_guid);
    }

    println!(
        "✓ {} items, {} enchanted — all parallel arrays valid",
        snapshot.len(),
        enchanted.len()
    );
}
