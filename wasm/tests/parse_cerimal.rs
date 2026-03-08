use nrftw_wasm::parser::{
    content::{children_of, find_node, node_meta, parse_document_content, ContentNode, ContentValue, PrimitiveValue},
    reader::Reader,
    schema::{fmt_type_ref, parse_schema, CerimalHeader, CerimalSchema},
    types::CompressionType,
};
use std::io::Read;

const CERIMAL: &str = "../examples/char_save_filled.cerimal";
const DICT: &str = "../cerimal_zstd.dict";

fn decompress(data: &[u8], dict: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    zstd::stream::read::Decoder::with_dictionary(data, dict)
        .expect("zstd init")
        .read_to_end(&mut out)
        .expect("zstd decompress");
    out
}

struct ParsedDocInfo {
    root_type: String,
    fields: Vec<(String, String, String)>,
}

fn parse_one_doc(data: &[u8], dict: &[u8], doc_idx: usize) -> ParsedDocInfo {
    let mut r = Reader::from_slice(data);

    for _ in 0..=doc_idx {
        assert!(&r.data[r.pos..r.pos + 7] == b"CERIMAL");
    }

    // Skip to the requested doc
    let mut current = 0;
    loop {
        assert!(r.remaining() >= 7, "not enough docs");
        assert!(&r.data[r.pos..r.pos + 7] == b"CERIMAL");

        let header = CerimalHeader::parse(&mut r).unwrap();

        if current == doc_idx {
            let schema_bytes = r.read_bytes(header.schema_size as usize).unwrap();
            let schema = parse_schema(&mut Reader::from_slice(&schema_bytes)).unwrap();
            let raw_content = r.read_bytes(header.content_size as usize).unwrap();
            let content_bytes = match header.comp_type {
                CompressionType::None => raw_content,
                CompressionType::Zstd => decompress(&raw_content, dict),
            };
            let root = parse_document_content(&schema, &content_bytes).unwrap();
            let root_type = fmt_type_ref(&schema.root_type_ref, &schema);
            let kids = children_of(&root, "", &schema);
            let fields = kids.into_iter().map(|c| {
                let val = c.meta.value.unwrap_or_default();
                (c.key, c.meta.type_str, val)
            }).collect();
            return ParsedDocInfo { root_type, fields };
        } else {
            // Skip this doc's schema + content
            r.read_bytes(header.schema_size as usize).unwrap();
            r.read_bytes(header.content_size as usize).unwrap();
        }
        current += 1;
    }
}

fn parse_save(path: &str, dict: &[u8]) -> Vec<(String, Vec<(String, String, String)>)> {
    let data = std::fs::read(path).expect("read cerimal");
    let mut results = Vec::new();
    let mut r = Reader::from_slice(&data);
    let mut doc_idx = 0;

    while r.remaining() >= 7 && &r.data[r.pos..r.pos + 7] == b"CERIMAL" {
        let header = CerimalHeader::parse(&mut r).unwrap();
        let schema_bytes = r.read_bytes(header.schema_size as usize).unwrap();
        let schema = parse_schema(&mut Reader::from_slice(&schema_bytes)).unwrap();
        let raw_content = r.read_bytes(header.content_size as usize).unwrap();
        let content_bytes = match header.comp_type {
            CompressionType::None => raw_content,
            CompressionType::Zstd => decompress(&raw_content, dict),
        };

        let root_type = fmt_type_ref(&schema.root_type_ref, &schema);

        match parse_document_content(&schema, &content_bytes) {
            Ok(root) => {
                let kids = children_of(&root, "", &schema);
                let fields = kids.into_iter().map(|c| {
                    let val = c.meta.value.unwrap_or_default();
                    (c.key, c.meta.type_str, val)
                }).collect();
                results.push((root_type, fields));
            }
            Err(e) => {
                eprintln!("Doc {} ({}) content parse error: {}", doc_idx, root_type, e);
                results.push((root_type, vec![]));
            }
        }
        doc_idx += 1;
    }
    results
}

#[test]
fn test_root_type() {
    let dict = std::fs::read(DICT).expect("read dict");
    let docs = parse_save(CERIMAL, &dict);
    assert!(!docs.is_empty(), "no documents parsed");
    let (root_type, _) = &docs[0];
    assert_eq!(root_type, "Quantum.CharacterMetadata", "root type mismatch: got {}", root_type);
    println!("✓ Doc 0 root type: {}", root_type);
}

#[test]
fn test_root_fields_exist() {
    let dict = std::fs::read(DICT).expect("read dict");
    let docs = parse_save(CERIMAL, &dict);
    let (_, fields) = &docs[0];

    // Print all root fields for inspection
    println!("Root fields ({}):", fields.len());
    for (k, t, v) in fields {
        println!("  {}: {} = {}", k, t, v);
    }

    let field_names: Vec<&str> = fields.iter().map(|(k, _, _)| k.as_str()).collect();
    assert!(!field_names.is_empty(), "no root fields found");

    // CharacterMetadata should have these at root level
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
    let dict = std::fs::read(DICT).expect("read dict");
    let docs = parse_save(CERIMAL, &dict);
    let (_, fields) = &docs[0];

    // Level, Health, Gold should be parseable integers
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
    let dict = std::fs::read(DICT).expect("read dict");
    let docs = parse_save(CERIMAL, &dict);
    let (_, fields) = &docs[0];

    let find = |name: &str| -> &str {
        &fields.iter().find(|(k, _, _)| k == name).unwrap().2
    };

    // Cross-check against Python parser output
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

    // FP: XP = 938.0 in Python (stored as fixed-point)
    let xp: f64 = find("XP").parse().unwrap();
    assert!((xp - 938.0).abs() < 0.01, "XP mismatch: {}", xp);

    println!("✓ All Doc 0 values match Python reference");
}

#[test]
fn test_exact_values_doc1() {
    let dict = std::fs::read(DICT).expect("read dict");
    let docs = parse_save(CERIMAL, &dict);
    assert!(docs.len() >= 2);
    let (_, fields) = &docs[1];

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
    let data = std::fs::read(CERIMAL).expect("read");
    let mut r2 = Reader::from_slice(&data);
    let h0 = CerimalHeader::parse(&mut r2).unwrap();
    r2.read_bytes(h0.schema_size as usize).unwrap();
    r2.read_bytes(h0.content_size as usize).unwrap();
    let h1 = CerimalHeader::parse(&mut r2).unwrap();
    let sb1 = r2.read_bytes(h1.schema_size as usize).unwrap();
    let schema1 = parse_schema(&mut Reader::from_slice(&sb1)).unwrap();
    let rc1 = r2.read_bytes(h1.content_size as usize).unwrap();
    let cb1 = decompress(&rc1, &dict);
    let root1 = parse_document_content(&schema1, &cb1).unwrap();

    let inv_node = find_node(&root1, "Inventory").expect("Inventory not found");
    let inv_children = children_of(inv_node, "Inventory", &schema1);
    assert_eq!(inv_children.len(), 108, "Inventory item count mismatch: got {}", inv_children.len());
    println!("  Inventory: {} items ✓", inv_children.len());

    println!("✓ All Doc 1 values match Python reference");
}

#[test]
fn test_doc1_character_contents() {
    let dict = std::fs::read(DICT).expect("read dict");
    let docs = parse_save(CERIMAL, &dict);
    assert!(docs.len() >= 2, "expected at least 2 documents, got {}", docs.len());

    let (root_type, fields) = &docs[1];
    assert_eq!(root_type, "Quantum.CharacterContents", "doc 1 root type mismatch: got {}", root_type);

    println!("Doc 1 root fields ({}):", fields.len());
    for (k, t, v) in fields {
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

const CATALOG_DB: &str = "../catalog.db";

/// Helper: parse doc 0, return (root, schema)
fn parse_doc0() -> (ContentNode, CerimalSchema) {
    let dict = std::fs::read(DICT).expect("read dict");
    let data = std::fs::read(CERIMAL).expect("read cerimal");
    let mut r = Reader::from_slice(&data);
    let header = CerimalHeader::parse(&mut r).unwrap();
    let schema_bytes = r.read_bytes(header.schema_size as usize).unwrap();
    let schema = parse_schema(&mut Reader::from_slice(&schema_bytes)).unwrap();
    let raw_content = r.read_bytes(header.content_size as usize).unwrap();
    let content_bytes = match header.comp_type {
        CompressionType::None => raw_content,
        CompressionType::Zstd => decompress(&raw_content, &dict),
    };
    let root = parse_document_content(&schema, &content_bytes).unwrap();
    (root, schema)
}

#[test]
fn test_equipment_guids_in_catalog() {
    let (root, schema) = parse_doc0();

    // Open catalog.db
    let db_path = std::path::Path::new(CATALOG_DB);
    if !db_path.exists() {
        eprintln!("Skipping catalog test: {} not found", CATALOG_DB);
        return;
    }

    // Expected equipment: field -> (assetguid, display_name)
    let expected: &[(&str, u64, &str)] = &[
        ("RightHand", 1023863541289577995, "Laquered Bow"),
        ("LeftHand",  716007893309524987,  "Wooden Shield"),
        ("Helmet",    1838235934543696561, "Wildling Helmet"),
        ("Body",      1797774859941529097, "Nith Leviathan Cloak"),
        ("Gloves",    3540398148196568207, "Bolein Rebels Fists"),
        ("Pants",     1698349693959893818, "Farmers Cords"),
    ];

    // Extract ASSETGUID from each equipment field via the Rust parser
    let mut extracted_guids: Vec<(&str, u64)> = Vec::new();
    for (slot, expected_guid, _) in expected {
        let id_path = format!("{}.Id", slot);
        let id_node = find_node(&root, &id_path)
            .unwrap_or_else(|| panic!("node '{}' not found", id_path));
        let meta = node_meta(id_node, &schema);
        assert_eq!(meta.type_str, "ASSETGUID", "{} type mismatch: {}", slot, meta.type_str);
        let guid_str = meta.guid.as_ref()
            .unwrap_or_else(|| panic!("{}.Id has no guid in meta", slot));
        let guid_val: u64 = guid_str.parse()
            .unwrap_or_else(|_| panic!("{}.Id guid '{}' is not a u64", slot, guid_str));
        assert_eq!(guid_val, *expected_guid, "{} ASSETGUID mismatch", slot);
        extracted_guids.push((slot, guid_val));
        println!("  {} ASSETGUID = {}", slot, guid_val);
    }

    // Look up each GUID in catalog.db via rusqlite... but we don't have rusqlite as a dep.
    // Use a subprocess to query sqlite3 CLI instead.
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
