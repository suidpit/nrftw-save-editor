use nrftw_wasm::document::{parse_all_docs, ParsedDoc};
use nrftw_wasm::dumper::{dump_all_docs, force_dump_doc_serialized, serialized_content_bytes, try_force_dump_all_docs};
use nrftw_wasm::parser::schema::fmt_type_ref;

const CERIMAL: &str = "../examples/char_save_filled.cerimal";
const DICT: &str = "../public/cerimal_zstd.dict";

fn load_fixture() -> (Vec<u8>, Vec<u8>) {
    let dict = std::fs::read(DICT).expect("read dict");
    let data = std::fs::read(CERIMAL).expect("read cerimal");
    (data, dict)
}

fn load_docs() -> Vec<ParsedDoc> {
    let (data, dict) = load_fixture();
    parse_all_docs(&data, &dict).expect("parse_all_docs failed")
}

fn root_types(docs: &[ParsedDoc]) -> Vec<String> {
    docs.iter()
        .map(|doc| fmt_type_ref(&doc.schema.root_type_ref, &doc.schema))
        .collect()
}

#[test]
fn fixture_parses_before_roundtrip_work() {
    let docs = load_docs();
    assert!(!docs.is_empty(), "fixture should parse into at least one document");

    let types = root_types(&docs);
    assert!(
        types.iter().any(|t| t == "Quantum.CharacterMetadata"),
        "expected CharacterMetadata doc, got {:?}",
        types
    );
    assert!(
        types.iter().any(|t| t == "Quantum.CharacterContents"),
        "expected CharacterContents doc, got {:?}",
        types
    );
}

#[test]
fn dump_roundtrips_doc_count_and_root_types() {
    let (_original_bytes, dict) = load_fixture();
    let docs = load_docs();
    let expected_types = root_types(&docs);

    let dumped = dump_all_docs(&docs, &dict);
    let reparsed = parse_all_docs(&dumped, &dict).expect("re-parse dumped docs");

    assert_eq!(reparsed.len(), docs.len(), "document count changed after dump");
    assert_eq!(
        root_types(&reparsed),
        expected_types,
        "root document types changed after dump"
    );
}

#[test]
fn untouched_docs_dump_stably() {
    let (original_bytes, dict) = load_fixture();
    let docs = parse_all_docs(&original_bytes, &dict).expect("parse original");

    let dumped = dump_all_docs(&docs, &dict);

    assert_eq!(
        dumped, original_bytes,
        "dumping untouched docs should be byte-stable for the fixture"
    );
}

#[test]
fn force_dump_preserves_parseability_and_matches_fixture() {
    let (original_bytes, dict) = load_fixture();
    let docs = parse_all_docs(&original_bytes, &dict).expect("parse original");

    let dumped = try_force_dump_all_docs(&docs, &dict).expect("force dump all docs");
    let reparsed = parse_all_docs(&dumped, &dict).expect("reparse force-dumped docs");

    assert_eq!(
        dumped, original_bytes,
        "force dumping should round-trip back to the exact fixture bytes"
    );
    assert_eq!(reparsed.len(), docs.len(), "force dump changed document count");
    assert_eq!(root_types(&reparsed), root_types(&docs), "force dump changed root types");
}

#[test]
fn force_dump_single_doc_preserves_other_doc_bytes() {
    let (original_bytes, dict) = load_fixture();
    let docs = parse_all_docs(&original_bytes, &dict).expect("parse original");

    let mut combined = Vec::new();
    combined.extend_from_slice(&docs[0].raw_bytes);
    combined.extend_from_slice(&force_dump_doc_serialized(&docs[1], &dict).expect("force dump doc 1"));

    let reparsed = parse_all_docs(&combined, &dict).expect("reparse mixed dump");

    assert_eq!(docs[0].raw_bytes, combined[..docs[0].raw_bytes.len()]);
    assert_eq!(reparsed.len(), docs.len(), "mixed dump changed document count");
    assert_eq!(root_types(&reparsed), root_types(&docs), "mixed dump changed root types");
}

#[test]
fn serialized_content_bytes_are_deterministic() {
    let docs = load_docs();

    let once = serialized_content_bytes(&docs[1]).expect("serialize content once");
    let twice = serialized_content_bytes(&docs[1]).expect("serialize content twice");

    assert_eq!(once, twice, "serialized content bytes should be deterministic");
    assert!(!once.is_empty(), "serialized content should not be empty");
}
