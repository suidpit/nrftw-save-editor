use super::reader::Reader;
use super::schema::{
    CerimalSchema, ClassMember, TypeDefMembers,
    compute_blittable_layout, compute_unmanaged_fields,
    fmt_type_ref, needs_tag, resolve_type_arg,
};
use super::types::{CerimalTypeReference, TypeDefinitionKind, WellKnownType};

// ---------------------------------------------------------------------------
// Content node tree
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum PrimitiveValue {
    Bool(bool),
    I64(i64),
    U64(u64),
    F64(f64),
    Fp(f64),   // fixed-point, stored as f64 after divide by 65536
    Lfp(f64),  // fixed-point i32, stored as f64 after divide by 65536
    Str(String),
    Guid([u8; 16]),
    Bytes(Vec<u8>),   // for DECIMAL
}

impl PrimitiveValue {
    pub fn to_display_string(&self) -> String {
        match self {
            Self::Bool(v) => if *v { "true".to_string() } else { "false".to_string() },
            Self::I64(v) => v.to_string(),
            Self::U64(v) => v.to_string(),
            Self::F64(v) => format!("{:.6}", v).trim_end_matches('0').trim_end_matches('.').to_string(),
            Self::Fp(v) => format!("{:.6}", v).trim_end_matches('0').trim_end_matches('.').to_string(),
            Self::Lfp(v) => format!("{:.6}", v).trim_end_matches('0').trim_end_matches('.').to_string(),
            Self::Str(v) => format!("{:?}", v),
            Self::Guid(b) => super::schema::format_guid_le(b),
            Self::Bytes(b) => {
                let hex: String = b.iter().map(|x| format!("{:02x}", x)).collect();
                hex
            }
        }
    }
}

#[derive(Debug, Clone)]
pub enum ContentValue {
    Primitive(PrimitiveValue),
    Composite(Vec<(String, ContentNode)>),
    Array(Vec<ContentNode>),
    Enum(i64),
}

#[derive(Debug, Clone)]
pub struct ConcreteNode {
    pub type_ref: CerimalTypeReference,
    pub value: ContentValue,
    pub value_start: usize,
    pub value_end: usize,
}

#[derive(Debug, Clone)]
pub enum ContentNode {
    Null(CerimalTypeReference),
    Backref {
        type_ref: CerimalTypeReference,
        index: u32,
    },
    Concrete(Box<ConcreteNode>),
    Polymorphic {
        declared_type: CerimalTypeReference,
        runtime_type: CerimalTypeReference,
        inner: Box<ContentNode>,
    },
}

impl ContentNode {
    /// Unwrap Polymorphic wrapper. Returns the innermost non-polymorphic node.
    pub fn unwrap_poly(&self) -> &ContentNode {
        let mut node = self;
        while let ContentNode::Polymorphic { inner, .. } = node {
            node = inner;
        }
        node
    }

    /// Returns effective type_ref (from polymorphic runtime type if applicable).
    pub fn type_ref(&self) -> &CerimalTypeReference {
        match self {
            ContentNode::Null(tr) => tr,
            ContentNode::Backref { type_ref, .. } => type_ref,
            ContentNode::Concrete(n) => &n.type_ref,
            ContentNode::Polymorphic { runtime_type, .. } => runtime_type,
        }
    }
}

// ---------------------------------------------------------------------------
// Parse context
// ---------------------------------------------------------------------------

pub struct ParseContext {
    pub backref_list: Vec<ContentNode>,
}

impl ParseContext {
    pub fn new() -> Self {
        Self { backref_list: Vec::new() }
    }

    pub fn register(&mut self, node: ContentNode) {
        self.backref_list.push(node);
    }
}

// ---------------------------------------------------------------------------
// Main content parsing entry point
// ---------------------------------------------------------------------------

pub fn parse_document_content(
    schema: &CerimalSchema,
    content_bytes: &[u8],
) -> Result<ContentNode, String> {
    let mut r = Reader::from_slice(content_bytes);
    let mut ctx = ParseContext::new();

    // Skip backref offset table (we only need the count for list capacity)
    let _backref_count = r.read_7bit_int()?;
    // The backref table stores offsets; we don't need them for sequential parse
    for _ in 0.._backref_count {
        r.read_7bit_int()?;
    }

    parse_node(&schema.root_type_ref, schema, &mut ctx, &mut r, None)
}

// ---------------------------------------------------------------------------
// Node parsing
// ---------------------------------------------------------------------------

fn parse_node(
    type_ref: &CerimalTypeReference,
    schema: &CerimalSchema,
    ctx: &mut ParseContext,
    r: &mut Reader,
    type_args: Option<&[CerimalTypeReference]>,
) -> Result<ContentNode, String> {
    let resolved = resolve_type_arg(type_ref, type_args);

    if !needs_tag(resolved, schema) {
        // Value types have no tag — parse value directly
        let val_start = r.pos;
        let value = parse_value(resolved, schema, ctx, r, 0, type_args)?;
        let val_end = r.pos;
        return Ok(ContentNode::Concrete(Box::new(ConcreteNode {
            type_ref: resolved.clone(),
            value,
            value_start: val_start,
            value_end: val_end,
        })));
    }

    // Reference type: read tag
    let tag = r.read_7bit_int()?;
    let kind = tag & 0x3;

    match kind {
        0 => Ok(ContentNode::Null(resolved.clone())),
        1 => {
            // Backref: bits[31:2] = index
            let index = tag >> 2;
            Ok(ContentNode::Backref { type_ref: resolved.clone(), index })
        }
        2 => {
            // Concrete with optional register bit
            let register_bit = (tag >> 2) & 1;
            let tag_payload = tag >> 3;
            let val_start = r.pos;
            let value = parse_value(resolved, schema, ctx, r, tag_payload, type_args)?;
            let val_end = r.pos;
            let node = ContentNode::Concrete(Box::new(ConcreteNode {
                type_ref: resolved.clone(),
                value,
                value_start: val_start,
                value_end: val_end,
            }));
            if register_bit != 0 {
                ctx.register(node.clone());
            }
            Ok(node)
        }
        3 => {
            // Polymorphic: runtime type follows
            let runtime_type = parse_type_reference_from_reader(r)?;
            let inner = parse_node(&runtime_type, schema, ctx, r, type_args)?;
            Ok(ContentNode::Polymorphic {
                declared_type: resolved.clone(),
                runtime_type,
                inner: Box::new(inner),
            })
        }
        _ => unreachable!(),
    }
}

fn parse_type_reference_from_reader(r: &mut Reader) -> Result<CerimalTypeReference, String> {
    super::schema::parse_type_reference(r)
}

// ---------------------------------------------------------------------------
// Value parsing
// ---------------------------------------------------------------------------

fn parse_value(
    type_ref: &CerimalTypeReference,
    schema: &CerimalSchema,
    ctx: &mut ParseContext,
    r: &mut Reader,
    tag_payload: u32,
    type_args: Option<&[CerimalTypeReference]>,
) -> Result<ContentValue, String> {
    match type_ref {
        CerimalTypeReference::Primitive(wkt) => {
            Ok(ContentValue::Primitive(parse_primitive(*wkt, r, tag_payload)?))
        }
        CerimalTypeReference::Array { rank, element } => {
            // tag_payload = first dimension; extra dims follow as 7-bit ints
            let mut dims = vec![tag_payload as usize];
            for _ in 1..(*rank as usize) {
                dims.push(r.read_7bit_int()? as usize);
            }
            let total: usize = dims.iter().product();
            let mut elements = Vec::with_capacity(total.min(100_000));
            for _ in 0..total {
                elements.push(parse_node(element, schema, ctx, r, type_args)?);
            }
            Ok(ContentValue::Array(elements))
        }
        CerimalTypeReference::TypeArgument { index } => {
            let args = type_args.ok_or("TypeArgument without type_args")?;
            let resolved = args.get(*index as usize)
                .ok_or_else(|| format!("TypeArgument index {} out of range", index))?;
            parse_value(resolved, schema, ctx, r, tag_payload, type_args)
        }
        CerimalTypeReference::Named { guid, type_args: type_refs } => {
            let idx = schema.guid_map.get(guid).copied()
                .ok_or_else(|| format!("Unknown type GUID: {} (schema has {} types)", super::schema::format_guid_le(guid), schema.type_defs.len()))?;
            let typedef = &schema.type_defs[idx];

            let effective_args: Option<&[CerimalTypeReference]> = if !type_refs.is_empty() {
                Some(type_refs)
            } else {
                type_args
            };

            match &typedef.members {
                TypeDefMembers::Enum(em) => {
                    let v = r.unpack_enum_type(&em.underlying_type)?;
                    Ok(ContentValue::Enum(v))
                }
                TypeDefMembers::StructLike(_) if typedef.kind == TypeDefinitionKind::Unmanaged
                    || typedef.kind == TypeDefinitionKind::Struct =>
                {
                    // Try blittable path first
                    match compute_unmanaged_fields(typedef, schema, effective_args) {
                        Ok((blob_size, unmanaged_fields)) => {
                            // Read full blob, create field nodes with precise byte offsets
                            let blob_start = r.pos;
                            let blob = r.read_bytes(blob_size)?;
                            let mut fields = Vec::new();
                            for (fname, field_offset, field_type_ref) in unmanaged_fields {
                                let (field_size, _) = compute_blittable_layout(
                                    field_type_ref, schema, effective_args
                                )?;
                                let mut field_reader = Reader::from_slice(&blob[field_offset..]);
                                let prim_val = parse_value(
                                    field_type_ref, schema, ctx,
                                    &mut field_reader, 0, effective_args
                                )?;
                                let node = ContentNode::Concrete(Box::new(ConcreteNode {
                                    type_ref: field_type_ref.clone(),
                                    value: prim_val,
                                    value_start: blob_start + field_offset,
                                    value_end: blob_start + field_offset + field_size,
                                }));
                                fields.push((fname.to_string(), node));
                            }
                            Ok(ContentValue::Composite(fields))
                        }
                        Err(_) => {
                            // Non-blittable: parse field-by-field
                            let mut fields = Vec::new();
                            read_record_fields_into(typedef, schema, ctx, r, effective_args, &mut fields)?;
                            Ok(ContentValue::Composite(fields))
                        }
                    }
                }
                TypeDefMembers::Class(_) | TypeDefMembers::StructLike(_) => {
                    let mut fields = Vec::new();
                    read_record_fields_into(typedef, schema, ctx, r, effective_args, &mut fields)?;
                    Ok(ContentValue::Composite(fields))
                }
            }
        }
    }
}

fn parse_primitive(
    wkt: WellKnownType,
    r: &mut Reader,
    tag_payload: u32,
) -> Result<PrimitiveValue, String> {
    match wkt {
        WellKnownType::String => {
            let len = tag_payload as usize;
            let bytes = r.read_bytes(len)?;
            Ok(PrimitiveValue::Str(String::from_utf8(bytes).unwrap_or_default()))
        }
        WellKnownType::Guid => {
            let b = r.read_bytes(16)?;
            Ok(PrimitiveValue::Guid(b.try_into().unwrap()))
        }
        WellKnownType::Decimal => {
            Ok(PrimitiveValue::Bytes(r.read_bytes(16)?))
        }
        WellKnownType::Fp => {
            let raw = i64::from_le_bytes(r.read_bytes(8)?.try_into().unwrap());
            Ok(PrimitiveValue::Fp(raw as f64 / 65536.0))
        }
        WellKnownType::Lfp => {
            let raw = i32::from_le_bytes(r.read_bytes(4)?.try_into().unwrap());
            Ok(PrimitiveValue::Lfp(raw as f64 / 65536.0))
        }
        WellKnownType::Bool => Ok(PrimitiveValue::Bool(r.u8()? != 0)),
        WellKnownType::SByte => Ok(PrimitiveValue::I64(r.u8()? as i8 as i64)),
        WellKnownType::Byte => Ok(PrimitiveValue::U64(r.u8()? as u64)),
        WellKnownType::Short => {
            let v = i16::from_le_bytes(r.read_bytes(2)?.try_into().unwrap());
            Ok(PrimitiveValue::I64(v as i64))
        }
        WellKnownType::UShort | WellKnownType::Char => {
            let v = u16::from_le_bytes(r.read_bytes(2)?.try_into().unwrap());
            Ok(PrimitiveValue::U64(v as u64))
        }
        WellKnownType::Int => {
            let v = i32::from_le_bytes(r.read_bytes(4)?.try_into().unwrap());
            Ok(PrimitiveValue::I64(v as i64))
        }
        WellKnownType::UInt => {
            let v = u32::from_le_bytes(r.read_bytes(4)?.try_into().unwrap());
            Ok(PrimitiveValue::U64(v as u64))
        }
        WellKnownType::Long | WellKnownType::NInt => {
            let v = i64::from_le_bytes(r.read_bytes(8)?.try_into().unwrap());
            Ok(PrimitiveValue::I64(v))
        }
        WellKnownType::ULong | WellKnownType::NUInt | WellKnownType::AssetGuid => {
            let v = u64::from_le_bytes(r.read_bytes(8)?.try_into().unwrap());
            Ok(PrimitiveValue::U64(v))
        }
        WellKnownType::Float => {
            let v = f32::from_le_bytes(r.read_bytes(4)?.try_into().unwrap());
            Ok(PrimitiveValue::F64(v as f64))
        }
        WellKnownType::Double => {
            let v = f64::from_le_bytes(r.read_bytes(8)?.try_into().unwrap());
            Ok(PrimitiveValue::F64(v))
        }
        WellKnownType::Unknown(_) => {
            Err(format!("Cannot parse unknown WellKnownType"))
        }
    }
}

// ---------------------------------------------------------------------------
// Record field reading (CLASS + STRUCT)
// ---------------------------------------------------------------------------

fn read_record_fields_into(
    typedef: &super::schema::CerimalTypeDefinition,
    schema: &CerimalSchema,
    ctx: &mut ParseContext,
    r: &mut Reader,
    type_args: Option<&[CerimalTypeReference]>,
    fields: &mut Vec<(String, ContentNode)>,
) -> Result<(), String> {
    // For CLASS: recurse into base class fields first
    if typedef.kind == TypeDefinitionKind::Class {
        if let Some(base_info) = &typedef.base_info {
            if base_info.has_base {
                if let Some(base_ref) = &base_info.base_type_ref {
                    let resolved_base = resolve_type_arg(base_ref, type_args);
                    if let CerimalTypeReference::Named { guid, type_args: type_refs } = resolved_base {
                        if let Some(&base_idx) = schema.guid_map.get(guid) {
                            // Resolve each type arg in the base reference against the parent's type_args
                            // (mirrors Python: base_args = [resolve_type_arg(a, type_args) for a in base_ref.type_refs])
                            let resolved_base_args: Vec<CerimalTypeReference>;
                            let base_args: Option<&[CerimalTypeReference]> = if !type_refs.is_empty() {
                                resolved_base_args = type_refs.iter()
                                    .map(|a| resolve_type_arg(a, type_args).clone())
                                    .collect();
                                Some(&resolved_base_args)
                            } else {
                                type_args
                            };
                            read_record_fields_into(
                                &schema.type_defs[base_idx],
                                schema, ctx, r, base_args, fields
                            )?;
                        }
                    }
                }
            }
        }
    }

    match &typedef.members {
        TypeDefMembers::Class(members) => {
            for member in members {
                match member {
                    ClassMember::Field { name, type_ref } => {
                        let node = parse_node(type_ref, schema, ctx, r, type_args)?;
                        fields.push((name.clone(), node));
                    }
                    ClassMember::List { element_type } => {
                        let count = r.read_7bit_int()? as usize;
                        let mut elems = Vec::with_capacity(count.min(10_000));
                        for _ in 0..count {
                            elems.push(parse_node(element_type, schema, ctx, r, type_args)?);
                        }
                        let node = ContentNode::Concrete(Box::new(ConcreteNode {
                            type_ref: CerimalTypeReference::Array {
                                rank: 1,
                                element: Box::new(element_type.clone()),
                            },
                            value: ContentValue::Array(elems),
                            value_start: 0,
                            value_end: 0,
                        }));
                        fields.push(("$list".to_string(), node));
                    }
                    ClassMember::Dict { key_type, value_type } => {
                        let count = r.read_7bit_int()? as usize;
                        let mut pairs = Vec::with_capacity(count.min(10_000) * 2);
                        for i in 0..count {
                            let k = parse_node(key_type, schema, ctx, r, type_args)?;
                            let v = parse_node(value_type, schema, ctx, r, type_args)?;
                            pairs.push((format!("$key{}", i), k));
                            pairs.push((format!("$val{}", i), v));
                        }
                        let node = ContentNode::Concrete(Box::new(ConcreteNode {
                            type_ref: CerimalTypeReference::TypeArgument { index: 0 }, // placeholder
                            value: ContentValue::Composite(pairs),
                            value_start: 0,
                            value_end: 0,
                        }));
                        fields.push(("$dict".to_string(), node));
                    }
                }
            }
        }
        TypeDefMembers::StructLike(members) => {
            for member in members {
                let node = parse_node(&member.type_ref, schema, ctx, r, type_args)?;
                fields.push((member.name.clone(), node));
            }
        }
        TypeDefMembers::Enum(_) => {
            return Err("read_record_fields on enum typedef".to_string());
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Node metadata (for get_node_children JSON output)
// ---------------------------------------------------------------------------

pub struct NodeMeta {
    pub type_str: String,
    pub is_leaf: bool,
    pub value: Option<String>,
    pub child_count: usize,
    pub guid: Option<String>,  // set for ASSETGUID nodes
}

pub fn node_meta(node: &ContentNode, schema: &CerimalSchema) -> NodeMeta {
    let actual = node.unwrap_poly();
    let type_str = fmt_type_ref(actual.type_ref(), schema);

    match actual {
        ContentNode::Null(_) => NodeMeta {
            type_str,
            is_leaf: true,
            value: Some("null".to_string()),
            child_count: 0,
            guid: None,
        },
        ContentNode::Backref { index, .. } => NodeMeta {
            type_str,
            is_leaf: true,
            value: Some(format!("→#{}", index)),
            child_count: 0,
            guid: None,
        },
        ContentNode::Concrete(n) => {
            match &n.value {
                ContentValue::Primitive(pv) => {
                    let is_asset_guid = matches!(&n.type_ref,
                        CerimalTypeReference::Primitive(WellKnownType::AssetGuid));
                    let val_str = pv.to_display_string();
                    let guid = if is_asset_guid {
                        if let PrimitiveValue::U64(v) = pv {
                            Some(v.to_string())
                        } else { None }
                    } else { None };
                    NodeMeta { type_str, is_leaf: true, value: Some(val_str), child_count: 0, guid }
                }
                ContentValue::Composite(fields) => NodeMeta {
                    type_str,
                    is_leaf: false,
                    value: None,
                    child_count: fields.len(),
                    guid: None,
                },
                ContentValue::Array(elems) => NodeMeta {
                    type_str,
                    is_leaf: false,
                    value: None,
                    child_count: elems.len(),
                    guid: None,
                },
                ContentValue::Enum(v) => NodeMeta {
                    type_str,
                    is_leaf: true,
                    value: Some(v.to_string()),
                    child_count: 0,
                    guid: None,
                },
            }
        }
        ContentNode::Polymorphic { .. } => unreachable!("unwrap_poly should eliminate this"),
    }
}

// ---------------------------------------------------------------------------
// Children enumeration (for get_node_children)
// ---------------------------------------------------------------------------

pub struct ChildEntry {
    pub key: String,
    pub path: String,
    pub meta: NodeMeta,
}

pub fn children_of(node: &ContentNode, path_prefix: &str, schema: &CerimalSchema) -> Vec<ChildEntry> {
    let actual = node.unwrap_poly();

    match actual {
        ContentNode::Concrete(n) => {
            match &n.value {
                ContentValue::Composite(fields) => {
                    fields.iter().map(|(fname, child)| {
                        let child_path = if path_prefix.is_empty() {
                            fname.clone()
                        } else {
                            format!("{}.{}", path_prefix, fname)
                        };
                        ChildEntry {
                            key: fname.clone(),
                            path: child_path,
                            meta: node_meta(child, schema),
                        }
                    }).collect()
                }
                ContentValue::Array(elems) => {
                    let limit = elems.len().min(200);
                    let mut out: Vec<ChildEntry> = elems[..limit].iter().enumerate().map(|(i, elem)| {
                        let child_path = format!("{}[{}]", path_prefix, i);
                        ChildEntry {
                            key: i.to_string(),
                            path: child_path,
                            meta: node_meta(elem, schema),
                        }
                    }).collect();
                    if elems.len() > limit {
                        out.push(ChildEntry {
                            key: format!("…{} more", elems.len() - limit),
                            path: String::new(),
                            meta: NodeMeta {
                                type_str: String::new(),
                                is_leaf: true,
                                value: None,
                                child_count: 0,
                                guid: None,
                            },
                        });
                    }
                    out
                }
                _ => {
                    // Leaf: wrap in single entry
                    vec![ChildEntry {
                        key: "(value)".to_string(),
                        path: path_prefix.to_string(),
                        meta: node_meta(actual, schema),
                    }]
                }
            }
        }
        _ => {
            vec![ChildEntry {
                key: "(value)".to_string(),
                path: path_prefix.to_string(),
                meta: node_meta(actual, schema),
            }]
        }
    }
}

// ---------------------------------------------------------------------------
// Path navigation (find_node)
// ---------------------------------------------------------------------------

pub fn find_node<'a>(node: &'a ContentNode, path: &str) -> Option<&'a ContentNode> {
    if path.is_empty() {
        return Some(node);
    }

    let actual = node.unwrap_poly();

    // Split on first '.'
    let (head, rest_opt) = match path.find('.') {
        Some(dot_pos) => (&path[..dot_pos], Some(&path[dot_pos + 1..])),
        None => (path, None),
    };

    // Parse optional array index: name[N]
    let (field_name, idx) = parse_head(head);

    match actual {
        ContentNode::Concrete(n) => {
            match &n.value {
                ContentValue::Composite(fields) => {
                    let child = fields.iter().find(|(k, _)| k == field_name).map(|(_, v)| v)?;
                    let child = if let Some(i) = idx {
                        // Drill into array element
                        if let ContentNode::Concrete(cn) = child.unwrap_poly() {
                            if let ContentValue::Array(elems) = &cn.value {
                                elems.get(i)?
                            } else { return None; }
                        } else { return None; }
                    } else {
                        child
                    };
                    match rest_opt {
                        Some(rest) => find_node(child, rest),
                        None => Some(child),
                    }
                }
                ContentValue::Array(elems) => {
                    // path is like "[N]" or "N"
                    let i = field_name.trim_matches(|c| c == '[' || c == ']')
                        .parse::<usize>().ok()?;
                    let elem = elems.get(i)?;
                    match rest_opt {
                        Some(rest) => find_node(elem, rest),
                        None => Some(elem),
                    }
                }
                _ => None,
            }
        }
        _ => None,
    }
}

/// Parse "name[N]" → ("name", Some(N)) or "name" → ("name", None)
fn parse_head(head: &str) -> (&str, Option<usize>) {
    if let Some(bracket) = head.find('[') {
        let name = &head[..bracket];
        let idx_str = head[bracket + 1..].trim_end_matches(']');
        let idx = idx_str.parse().ok();
        (name, idx)
    } else {
        (head, None)
    }
}

// ---------------------------------------------------------------------------
// Primitive encoding (for patch_field)
// ---------------------------------------------------------------------------

pub fn encode_primitive(
    node: &ConcreteNode,
    value_str: &str,
) -> Result<(usize, Vec<u8>), String> {
    match &node.type_ref {
        CerimalTypeReference::Primitive(wkt) => {
            let bytes = encode_primitive_wkt(*wkt, value_str)?;
            Ok((node.value_start, bytes))
        }
        _ => Err("Not a primitive node".to_string()),
    }
}

fn encode_primitive_wkt(wkt: WellKnownType, value_str: &str) -> Result<Vec<u8>, String> {
    match wkt {
        WellKnownType::Fp => {
            let v: f64 = value_str.parse().map_err(|e: std::num::ParseFloatError| e.to_string())?;
            let raw = (v * 65536.0).round() as i64;
            Ok(raw.to_le_bytes().to_vec())
        }
        WellKnownType::Lfp => {
            let v: f64 = value_str.parse().map_err(|e: std::num::ParseFloatError| e.to_string())?;
            let raw = (v * 65536.0).round() as i32;
            Ok(raw.to_le_bytes().to_vec())
        }
        WellKnownType::Bool => {
            let v: bool = match value_str.trim() {
                "true" | "1" => true,
                "false" | "0" => false,
                _ => value_str.parse::<i64>().map(|v| v != 0).map_err(|_| format!("Invalid bool: {}", value_str))?,
            };
            Ok(vec![v as u8])
        }
        WellKnownType::SByte => {
            let v: i8 = value_str.parse().map_err(|e: std::num::ParseIntError| e.to_string())?;
            Ok(vec![v as u8])
        }
        WellKnownType::Byte => {
            let v: u8 = value_str.parse().map_err(|e: std::num::ParseIntError| e.to_string())?;
            Ok(vec![v])
        }
        WellKnownType::Short => {
            let v: i16 = value_str.parse().map_err(|e: std::num::ParseIntError| e.to_string())?;
            Ok(v.to_le_bytes().to_vec())
        }
        WellKnownType::UShort | WellKnownType::Char => {
            let v: u16 = value_str.parse().map_err(|e: std::num::ParseIntError| e.to_string())?;
            Ok(v.to_le_bytes().to_vec())
        }
        WellKnownType::Int => {
            let v: i32 = parse_int_or_float(value_str)? as i32;
            Ok(v.to_le_bytes().to_vec())
        }
        WellKnownType::UInt => {
            let v: u32 = parse_int_or_float(value_str)? as i64 as u32;
            Ok(v.to_le_bytes().to_vec())
        }
        WellKnownType::Long | WellKnownType::NInt => {
            let v: i64 = parse_int_or_float(value_str)?;
            Ok(v.to_le_bytes().to_vec())
        }
        WellKnownType::ULong | WellKnownType::NUInt => {
            let v: u64 = value_str.parse().map_err(|e: std::num::ParseIntError| e.to_string())?;
            Ok(v.to_le_bytes().to_vec())
        }
        WellKnownType::Float => {
            let v: f32 = value_str.parse().map_err(|e: std::num::ParseFloatError| e.to_string())?;
            Ok(v.to_le_bytes().to_vec())
        }
        WellKnownType::Double => {
            let v: f64 = value_str.parse().map_err(|e: std::num::ParseFloatError| e.to_string())?;
            Ok(v.to_le_bytes().to_vec())
        }
        WellKnownType::AssetGuid => {
            let v: u64 = value_str.parse().map_err(|e: std::num::ParseIntError| e.to_string())?;
            Ok(v.to_le_bytes().to_vec())
        }
        _ => Err(format!("Cannot encode primitive type: {}", wkt.name())),
    }
}

fn parse_int_or_float(s: &str) -> Result<i64, String> {
    if s.contains('.') {
        let f: f64 = s.parse().map_err(|e: std::num::ParseFloatError| e.to_string())?;
        Ok(f.round() as i64)
    } else {
        s.parse::<i64>().map_err(|e| e.to_string())
    }
}
