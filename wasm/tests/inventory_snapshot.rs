use nrftw_wasm::document::parse_all_docs;
use nrftw_wasm::inventory_snapshot;

const CERIMAL: &str = "../examples/char_save_filled.cerimal";
const DICT: &str = "../public/cerimal_zstd.dict";

fn load_fixture() -> (Vec<u8>, Vec<u8>) {
    let dict = std::fs::read(DICT).expect("read dict");
    let data = std::fs::read(CERIMAL).expect("read cerimal");
    (data, dict)
}

#[test]
fn inventory_snapshot_includes_equipment_and_enchantments() {
    let (data, dict) = load_fixture();
    let docs = parse_all_docs(&data, &dict).expect("parse fixture");
    let items = inventory_snapshot(&docs[1]).expect("snapshot");

    assert_eq!(items.len(), 108, "inventory size mismatch");

    let equipped = &items[0];
    assert_eq!(equipped.index, 0);
    assert_eq!(equipped.item_path, "Inventory[0]");
    assert_eq!(equipped.asset_guid, "2340426941660951149");
    assert_eq!(equipped.level, 4);
    assert_eq!(equipped.rarity_num, 0);
    assert_eq!(equipped.durability, -1);
    assert_eq!(equipped.stack_count, 8);
    assert_eq!(equipped.slot_num, -1);
    assert!(equipped.rune_guids.is_empty());
    assert!(equipped.enchantment_guids.is_empty());
    assert_eq!(equipped.trait_guid, "");

    let stacked = items
        .iter()
        .find(|item| item.stack_count > 1)
        .expect("fixture should contain stacked inventory");
    assert_eq!(stacked.item_path, "Inventory[0]");
    assert_eq!(stacked.stack_count, 8);

    let enchanted = items
        .iter()
        .find(|item| !item.enchantment_guids.is_empty())
        .expect("fixture should contain an enchanted item");
    assert_eq!(enchanted.item_path, "Inventory[1]");
    assert_eq!(
        enchanted
            .enchantment_guids,
        vec![
            "3202028461714202202".to_string(),
            "4020486372537380339".to_string(),
            "2824610576232940589".to_string(),
        ]
    );
    assert!(!enchanted.trait_guid.is_empty());

    let runed = items
        .iter()
        .find(|item| !item.rune_guids.is_empty())
        .expect("fixture should contain a runed weapon");
    assert!(!runed.rune_guids.is_empty());

    let equipped_slot = items
        .iter()
        .find(|item| item.slot_num >= 0)
        .expect("fixture should contain equipped inventory");
    assert_eq!(equipped_slot.item_path, "Inventory[16]");
    assert_eq!(equipped_slot.slot_num, 3);
}
