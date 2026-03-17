use super::reader::Reader;
use super::types::{CerimalTypeReference, TypeDefinitionKind, WellKnownType};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Schema data structures
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct CerimalHeader {
    pub magic: [u8; 7],
    pub version: u8,
    pub schema_size: u32,
    pub content_size: u32,
    pub comp_type: super::types::CompressionType,
    pub checksum: u64,
}

impl CerimalHeader {
    pub fn parse(r: &mut Reader<'_>) -> Result<Self, String> {
        let magic = r.read_array()?;
        if &magic != b"CERIMAL" {
            return Err(format!("Bad magic: {:?}", magic));
        }
        let version = r.u8()?;
        let schema_size = r.u32_le()?;
        let content_size = r.u32_le()?;
        let comp_type_raw = r.u32_le()?;
        let comp_type = if comp_type_raw == 0 {
            super::types::CompressionType::None
        } else {
            super::types::CompressionType::Zstd
        };
        let checksum = r.u64_le()?;
        Ok(Self {
            magic,
            version,
            schema_size,
            content_size,
            comp_type,
            checksum,
        })
    }
}

#[derive(Debug, Clone)]
pub struct CerimalBaseInfo {
    pub has_base: bool,
    pub base_type_ref: Option<CerimalTypeReference>,
    pub interface_count: u32,
}

impl CerimalBaseInfo {
    pub fn parse(r: &mut Reader<'_>) -> Result<Self, String> {
        // Packed u32 layout:
        //   bits  0-23: total byte size of this base-info section (including this u32)
        //   bit     24: has_base flag (1 = type has a base class)
        //   bits 25-31: number of implemented interfaces
        let raw = r.u32_le()?;
        let bsize = (raw & 0x00FF_FFFF) as usize;
        let has_base = (raw & 0x0100_0000) != 0;
        let interface_count = raw >> 25;
        // bsize includes the 4-byte raw field itself
        let body_len = bsize.saturating_sub(4);
        let body_bytes = r.read_slice(body_len)?;
        let mut body = Reader::from_slice(body_bytes);
        let base_type_ref = if has_base {
            Some(parse_type_reference(&mut body)?)
        } else {
            None
        };
        Ok(Self {
            has_base,
            base_type_ref,
            interface_count,
        })
    }
}

#[derive(Debug, Clone)]
pub enum ClassMember {
    List {
        element_type: CerimalTypeReference,
    },
    Dict {
        key_type: CerimalTypeReference,
        value_type: CerimalTypeReference,
    },
    Field {
        name: String,
        type_ref: CerimalTypeReference,
    },
}

#[derive(Debug, Clone)]
pub struct StructMember {
    pub name: String,
    pub type_ref: CerimalTypeReference,
}

pub type UnmanagedFieldLayout<'a> = (&'a str, usize, &'a CerimalTypeReference);

#[derive(Debug, Clone)]
pub struct EnumVariant {
    pub name: String,
    pub value: i64,
}

#[derive(Debug, Clone)]
pub struct EnumMembers {
    pub underlying_type: String,
    pub variants: Vec<EnumVariant>,
}

impl EnumMembers {
    pub fn type_size(&self) -> usize {
        match self.underlying_type.as_str() {
            "u8" | "i8" => 1,
            "u16" | "i16" => 2,
            "u32" | "i32" => 4,
            "u64" | "i64" => 8,
            _ => 4,
        }
    }
}

#[derive(Debug, Clone)]
pub enum TypeDefMembers {
    Class(Vec<ClassMember>),
    StructLike(Vec<StructMember>),
    Enum(EnumMembers),
}

#[derive(Debug, Clone)]
pub struct CerimalTypeDefinition {
    pub kind: TypeDefinitionKind,
    pub guid: [u8; 16],
    pub local_version: u32,
    pub name: String,
    pub base_info: Option<CerimalBaseInfo>,
    pub members: TypeDefMembers,
}

#[derive(Debug, Clone)]
pub struct CerimalSchema {
    pub type_defs: Vec<CerimalTypeDefinition>,
    pub root_type_ref: CerimalTypeReference,
    /// guid bytes → index in type_defs
    pub guid_map: HashMap<[u8; 16], usize>,
}

// ---------------------------------------------------------------------------
// Type reference parsing
// ---------------------------------------------------------------------------

pub fn parse_type_reference(r: &mut Reader<'_>) -> Result<CerimalTypeReference, String> {
    let header = r.u8()?;
    let kind = header & 0x3;
    match kind {
        0 => {
            let wkt = WellKnownType::from_u8(header >> 3);
            Ok(CerimalTypeReference::Primitive(wkt))
        }
        1 => {
            let rank = (header >> 3) as u32;
            let element = parse_type_reference(r)?;
            Ok(CerimalTypeReference::Array {
                rank,
                element: Box::new(element),
            })
        }
        2 => {
            let index = (header >> 3) as u32;
            Ok(CerimalTypeReference::TypeArgument { index })
        }
        3 => {
            let type_arg_count = (header >> 3) as usize;
            let guid = r.read_array()?;
            let mut type_args = Vec::with_capacity(type_arg_count);
            for _ in 0..type_arg_count {
                type_args.push(parse_type_reference(r)?);
            }
            Ok(CerimalTypeReference::Named { guid, type_args })
        }
        _ => unreachable!(),
    }
}

// ---------------------------------------------------------------------------
// Schema parsing
// ---------------------------------------------------------------------------

impl CerimalSchema {
    pub fn parse(r: &mut Reader<'_>) -> Result<Self, String> {
        // Schema header: global_version + type_defs_size + type_defs_count + name_strings_size
        let _global_version = r.u32_le()?;
        let _type_defs_size = r.u32_le()?;
        let type_defs_count = r.u32_le()? as usize;
        let _name_strings_size = r.u32_le()?;

        // Parse type definitions
        let mut type_defs: Vec<CerimalTypeDefinition> = Vec::with_capacity(type_defs_count);
        for _ in 0..type_defs_count {
            type_defs.push(parse_type_def(r)?);
        }

        // Parse name strings (one 7-bit-length-prefixed UTF-8 string per typedef)
        for type_def in type_defs.iter_mut().take(type_defs_count) {
            let name = r.read_7bit_name()?;
            type_def.name = name;
        }

        // Root type reference
        let root_type_ref = parse_type_reference(r)?;

        // Build guid → index map
        let mut guid_map = HashMap::new();
        for (i, td) in type_defs.iter().enumerate() {
            guid_map.insert(td.guid, i);
        }

        Ok(CerimalSchema {
            type_defs,
            root_type_ref,
            guid_map,
        })
    }
}

fn parse_type_def(r: &mut Reader<'_>) -> Result<CerimalTypeDefinition, String> {
    let td_start = r.pos;

    // Fixed 28-byte header: size(4) + flags(4) + local_version(4) + guid(16)
    let var_size = r.u32_le()? as usize; // size of variable part after the 28-byte fixed header
    let td_end = td_start + 28 + var_size;

    let flags = r.u32_le()?;
    let kind_raw = (flags & 0x3) as u8;
    let has_base_or_interfaces = (flags & 0x4) != 0;
    // type_parameter_count = (flags >> 3) & 0x1F  (not stored, only used during parse)
    let member_count = ((flags >> 8) & 0x00FF_FFFF) as usize;

    let local_version = r.u32_le()?;
    let guid = r.read_array()?;

    let kind = TypeDefinitionKind::from_u8(kind_raw)?;

    // Optional base/interfaces info
    let base_info = if has_base_or_interfaces {
        Some(CerimalBaseInfo::parse(r)?)
    } else {
        None
    };

    // Members, depending on kind
    let members = match kind {
        TypeDefinitionKind::Class => {
            let mut class_members = Vec::with_capacity(member_count);
            for _ in 0..member_count {
                class_members.push(parse_class_member(r)?);
            }
            TypeDefMembers::Class(class_members)
        }
        TypeDefinitionKind::Struct | TypeDefinitionKind::Unmanaged => {
            let mut struct_members = Vec::with_capacity(member_count);
            for _ in 0..member_count {
                let name = r.read_name()?;
                let type_ref = parse_type_reference(r)?;
                struct_members.push(StructMember { name, type_ref });
            }
            TypeDefMembers::StructLike(struct_members)
        }
        TypeDefinitionKind::Enum => {
            let m_packed = r.u8()?;
            let type_idx = m_packed & 0x7;
            let underlying_type = ENUM_TYPES[type_idx as usize].to_string();
            let mut variants = Vec::with_capacity(member_count);
            for _ in 0..member_count {
                let name = r.read_name()?;
                let value = r.unpack_enum_type(&underlying_type)?;
                variants.push(EnumVariant { name, value });
            }
            TypeDefMembers::Enum(EnumMembers {
                underlying_type,
                variants,
            })
        }
    };

    // Snap to exact end to handle any parsing drift
    r.pos = td_end;

    Ok(CerimalTypeDefinition {
        kind,
        guid,
        local_version,
        name: String::new(), // filled in post-processing
        base_info,
        members,
    })
}

fn parse_class_member(r: &mut Reader<'_>) -> Result<ClassMember, String> {
    let kind = r.u8()?;
    match kind {
        0 => {
            let element_type = parse_type_reference(r)?;
            Ok(ClassMember::List { element_type })
        }
        1 => {
            let key_type = parse_type_reference(r)?;
            let value_type = parse_type_reference(r)?;
            Ok(ClassMember::Dict {
                key_type,
                value_type,
            })
        }
        2 => {
            let name = r.read_name()?;
            let type_ref = parse_type_reference(r)?;
            Ok(ClassMember::Field { name, type_ref })
        }
        _ => Err(format!("Unknown class member kind: {}", kind)),
    }
}

static ENUM_TYPES: &[&str] = &["u8", "i8", "u16", "i16", "u32", "i32", "u64", "i64"];

// ---------------------------------------------------------------------------
// Type reference formatting
// ---------------------------------------------------------------------------

pub fn fmt_type_ref(type_ref: &CerimalTypeReference, schema: &CerimalSchema) -> String {
    match type_ref {
        CerimalTypeReference::Primitive(wkt) => wkt.name().to_string(),
        CerimalTypeReference::Array { rank, element } => {
            let dims = ",".repeat((*rank as usize).saturating_sub(1));
            format!("{}[{}]", fmt_type_ref(element, schema), dims)
        }
        CerimalTypeReference::TypeArgument { index } => format!("T{}", index),
        CerimalTypeReference::Named { guid, type_args } => {
            let name = if let Some(&idx) = schema.guid_map.get(guid) {
                schema.type_defs[idx].name.clone()
            } else {
                super::types::format_guid_le(guid)
            };
            if type_args.is_empty() {
                name
            } else {
                let args: Vec<String> = type_args.iter().map(|a| fmt_type_ref(a, schema)).collect();
                format!("{}<{}>", name, args.join(", "))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Needs-tag check (mirrors Python needs_tag)
// ---------------------------------------------------------------------------

/// Returns true if this type requires a node tag in the content stream.
/// Value types (non-string primitives, structs, unmanaged, enums) have no tag.
pub fn needs_tag(type_ref: &CerimalTypeReference, schema: &CerimalSchema) -> bool {
    match type_ref {
        CerimalTypeReference::Primitive(wkt) => wkt.is_reference(),
        CerimalTypeReference::Array { .. } => true,
        CerimalTypeReference::Named { guid, .. } => {
            if let Some(&idx) = schema.guid_map.get(guid) {
                schema.type_defs[idx].kind == TypeDefinitionKind::Class
            } else {
                true // conservative
            }
        }
        CerimalTypeReference::TypeArgument { .. } => true, // conservative
    }
}

// ---------------------------------------------------------------------------
// Type argument resolution
// ---------------------------------------------------------------------------

pub fn resolve_type_arg<'a>(
    type_ref: &'a CerimalTypeReference,
    type_args: Option<&'a [CerimalTypeReference]>,
) -> &'a CerimalTypeReference {
    if let CerimalTypeReference::TypeArgument { index } = type_ref {
        if let Some(args) = type_args {
            if (*index as usize) < args.len() {
                return &args[*index as usize];
            }
        }
    }
    type_ref
}

// ---------------------------------------------------------------------------
// Blittable layout computation (mirrors Python compute_blittable_layout)
// ---------------------------------------------------------------------------

/// Returns (size, alignment) for a blittable type, or Err if not blittable.
pub fn compute_blittable_layout(
    type_ref: &CerimalTypeReference,
    schema: &CerimalSchema,
    type_args: Option<&[CerimalTypeReference]>,
) -> Result<(usize, usize), String> {
    match type_ref {
        CerimalTypeReference::Primitive(wkt) => {
            let (size, align) = wkt.size_align();
            if size == 0 {
                Err(format!("Primitive type is not blittable: {}", wkt.name()))
            } else {
                Ok((size, align))
            }
        }
        CerimalTypeReference::TypeArgument { index } => {
            let args = type_args.ok_or("TypeArgument without type_args")?;
            let resolved = args
                .get(*index as usize)
                .ok_or_else(|| format!("TypeArgument index {} out of range", index))?;
            compute_blittable_layout(resolved, schema, type_args)
        }
        CerimalTypeReference::Array { .. } => Err("Array type is not blittable".to_string()),
        CerimalTypeReference::Named {
            guid,
            type_args: named_type_args,
        } => {
            let idx = schema
                .guid_map
                .get(guid)
                .copied()
                .ok_or_else(|| "Unknown GUID in blittable layout".to_string())?;
            let typedef = &schema.type_defs[idx];
            let effective_args: Option<&[CerimalTypeReference]> = if !named_type_args.is_empty() {
                Some(named_type_args)
            } else {
                type_args
            };
            match typedef.kind {
                TypeDefinitionKind::Enum => {
                    if let TypeDefMembers::Enum(em) = &typedef.members {
                        let sz = em.type_size();
                        Ok((sz, sz))
                    } else {
                        Err("Enum typedef without EnumMembers".to_string())
                    }
                }
                TypeDefinitionKind::Unmanaged | TypeDefinitionKind::Struct => {
                    if let TypeDefMembers::StructLike(members) = &typedef.members {
                        let mut offset = 0usize;
                        let mut max_align = 1usize;
                        for m in members {
                            let resolved = resolve_type_arg(&m.type_ref, effective_args);
                            let (sz, al) =
                                compute_blittable_layout(resolved, schema, effective_args)?;
                            offset = align_up(offset, al);
                            offset += sz;
                            max_align = max_align.max(al);
                        }
                        let total = align_up(offset, max_align);
                        Ok((total, max_align))
                    } else {
                        Err("StructLike typedef without StructLike members".to_string())
                    }
                }
                TypeDefinitionKind::Class => Err("Class type is not blittable".to_string()),
            }
        }
    }
}

/// Returns (total_size, Vec<(name, offset, type_ref)>) for an unmanaged/struct typedef.
pub fn compute_unmanaged_fields<'a>(
    typedef: &'a CerimalTypeDefinition,
    schema: &CerimalSchema,
    type_args: Option<&'a [CerimalTypeReference]>,
) -> Result<(usize, Vec<UnmanagedFieldLayout<'a>>), String> {
    let members = if let TypeDefMembers::StructLike(m) = &typedef.members {
        m
    } else {
        return Err("Not a struct-like typedef".to_string());
    };

    let mut offset = 0usize;
    let mut max_align = 1usize;
    let mut fields = Vec::new();

    for m in members {
        let resolved = resolve_type_arg(&m.type_ref, type_args);
        let (sz, al) = compute_blittable_layout(resolved, schema, type_args)?;
        offset = align_up(offset, al);
        fields.push((m.name.as_str(), offset, resolved));
        offset += sz;
        max_align = max_align.max(al);
    }

    let total = align_up(offset, max_align);
    Ok((total, fields))
}

pub fn align_up(offset: usize, align: usize) -> usize {
    if align == 0 {
        return offset;
    }
    (offset + align - 1) & !(align - 1)
}
