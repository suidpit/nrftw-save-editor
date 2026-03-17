use crate::parser::content::{encode_primitive, find_node, ContentNode};
use crate::parser::reader::Reader;
use crate::parser::schema::{CerimalHeader, CerimalSchema};
use crate::parser::types::CompressionType;

pub struct ParsedDoc {
    pub header: CerimalHeader,
    pub raw_bytes: Vec<u8>,
    pub schema_bytes: Vec<u8>,
    pub content_bytes: Vec<u8>, // decompressed content
    pub comp_type: CompressionType,
    pub schema: CerimalSchema,
    pub root: ContentNode,
}

pub fn decompress_content(
    data: &[u8],
    comp_type: CompressionType,
    dict_bytes: &[u8],
) -> Result<Vec<u8>, String> {
    match comp_type {
        CompressionType::None => Ok(data.to_vec()),
        CompressionType::Zstd => {
            use std::io::Read;
            let mut out = Vec::new();
            zstd::stream::read::Decoder::with_dictionary(data, dict_bytes)
                .map_err(|e| format!("zstd init: {}", e))?
                .read_to_end(&mut out)
                .map_err(|e| format!("zstd decompress: {}", e))?;
            Ok(out)
        }
    }
}

pub fn parse_all_docs(data: &[u8], dict_bytes: &[u8]) -> Result<Vec<ParsedDoc>, String> {
    let mut r = Reader::from_slice(data);
    let mut docs = Vec::new();

    while r.remaining() >= 7 {
        if &r.data[r.pos..r.pos + 7] != b"CERIMAL" {
            break;
        }

        let doc_start = r.pos;
        let header = CerimalHeader::parse(&mut r).map_err(|e| e.to_string())?;
        let schema_bytes = r
            .read_slice(header.schema_size as usize)
            .map_err(|e| e.to_string())?;
        let schema = CerimalSchema::parse(&mut Reader::from_slice(schema_bytes))
            .map_err(|e| e.to_string())?;

        let raw_content = r
            .read_slice(header.content_size as usize)
            .map_err(|e| e.to_string())?;
        let doc_end = r.pos;

        let content_bytes = decompress_content(raw_content, header.comp_type, dict_bytes)?;
        let root =
            ContentNode::parse_document(&schema, &content_bytes).map_err(|e| e.to_string())?;
        let raw_bytes = data[doc_start..doc_end].to_vec();

        docs.push(ParsedDoc {
            header: header.clone(),
            raw_bytes,
            schema_bytes: schema_bytes.to_vec(),
            content_bytes,
            comp_type: header.comp_type,
            schema,
            root,
        });
    }

    if docs.is_empty() {
        return Err("no CERIMAL documents found".to_string());
    }

    Ok(docs)
}

pub fn rebuild_doc_bytes(doc: &ParsedDoc, compressed: &[u8]) -> Vec<u8> {
    let checksum =
        xxhash_rust::xxh64::xxh64(&[doc.schema_bytes.as_slice(), compressed].concat(), 0);

    let comp_type_int: u32 = if doc.comp_type == CompressionType::Zstd {
        1
    } else {
        0
    };
    let mut new_header = Vec::with_capacity(28);
    new_header.extend_from_slice(&doc.header.magic);
    new_header.push(doc.header.version);
    new_header.extend_from_slice(&(doc.schema_bytes.len() as u32).to_le_bytes());
    new_header.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
    new_header.extend_from_slice(&comp_type_int.to_le_bytes());
    new_header.extend_from_slice(&checksum.to_le_bytes());

    let mut result = new_header;
    result.extend_from_slice(&doc.schema_bytes);
    result.extend_from_slice(compressed);
    result
}

/// Patch a primitive field in `doc` and return the rebuilt single-document bytes.
/// `compress` receives the new decompressed content and must return compressed bytes.
/// For uncompressed docs, pass `|data| Ok(data.to_vec())`.
pub fn patch_doc(
    doc: &ParsedDoc,
    field_name: &str,
    value_str: &str,
    compress: impl Fn(&[u8]) -> Result<Vec<u8>, String>,
) -> Result<Vec<u8>, String> {
    // Find node and encode the new value
    let node = find_node(&doc.root, field_name)
        .ok_or_else(|| format!("field '{}' not found", field_name))?;
    let actual = node.unwrap_poly();
    let (value_start, encoded) = if let ContentNode::Concrete(cn) = actual {
        encode_primitive(cn, value_str)?
    } else {
        return Err(format!("field '{}' is not a concrete node", field_name));
    };

    // Patch content bytes
    let mut content = doc.content_bytes.clone();
    let end = value_start + encoded.len();
    if end > content.len() {
        return Err(format!(
            "patch overflows content_bytes: {} + {} > {}",
            value_start,
            encoded.len(),
            content.len()
        ));
    }
    content[value_start..end].copy_from_slice(&encoded);

    // Recompress
    let compressed = compress(&content)?;

    Ok(rebuild_doc_bytes(doc, &compressed))
}
