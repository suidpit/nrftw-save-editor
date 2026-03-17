use std::collections::BTreeSet;
use std::io::Write;

use crate::document::{rebuild_doc_bytes, ParsedDoc};
use crate::parser::content::{ConcreteNode, ContentNode, ContentValue, PrimitiveValue};
use crate::parser::schema::{
    align_up, compute_blittable_layout, fmt_type_ref, needs_tag, resolve_type_arg, CerimalSchema,
    CerimalTypeDefinition, ClassMember, TypeDefMembers,
};
use crate::parser::types::{
    CerimalTypeReference, CompressionType, TypeDefinitionKind, WellKnownType,
};

pub fn dump_all_docs(docs: &[ParsedDoc], dict_bytes: &[u8]) -> Vec<u8> {
    try_dump_all_docs(docs, dict_bytes).expect("dump_all_docs failed")
}

pub fn try_dump_all_docs(docs: &[ParsedDoc], dict_bytes: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    for doc in docs {
        out.extend_from_slice(&dump_doc(doc, dict_bytes)?);
    }
    Ok(out)
}

pub fn try_force_dump_all_docs(docs: &[ParsedDoc], dict_bytes: &[u8]) -> Result<Vec<u8>, String> {
    try_force_dump_all_docs_with_compressor(docs, |comp_type, content| {
        compress_content(comp_type, content, dict_bytes)
    })
}

pub fn try_force_dump_all_docs_with_compressor(
    docs: &[ParsedDoc],
    compress: impl Fn(CompressionType, &[u8]) -> Result<Vec<u8>, String>,
) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    for doc in docs {
        out.extend_from_slice(&dump_doc_with_compressor(doc, &compress, true)?);
    }
    Ok(out)
}

pub fn try_dump_all_docs_with_dirty(
    docs: &[ParsedDoc],
    dict_bytes: &[u8],
    dirty_docs: &BTreeSet<usize>,
) -> Result<Vec<u8>, String> {
    try_dump_all_docs_with_dirty_and_compressor(docs, dirty_docs, |comp_type, content| {
        compress_content(comp_type, content, dict_bytes)
    })
}

pub fn try_dump_all_docs_with_dirty_and_compressor(
    docs: &[ParsedDoc],
    dirty_docs: &BTreeSet<usize>,
    compress: impl Fn(CompressionType, &[u8]) -> Result<Vec<u8>, String>,
) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    for (idx, doc) in docs.iter().enumerate() {
        let bytes = if dirty_docs.contains(&idx) {
            dump_doc_with_compressor(doc, &compress, false)?
        } else {
            doc.raw_bytes.clone()
        };
        out.extend_from_slice(&bytes);
    }
    Ok(out)
}

pub fn dump_doc(doc: &ParsedDoc, dict_bytes: &[u8]) -> Result<Vec<u8>, String> {
    if !doc.raw_bytes.is_empty() {
        return Ok(doc.raw_bytes.clone());
    }

    dump_doc_serialized(doc, dict_bytes)
}

pub fn dump_doc_serialized(doc: &ParsedDoc, dict_bytes: &[u8]) -> Result<Vec<u8>, String> {
    force_dump_doc_serialized(doc, dict_bytes)
}

pub fn force_dump_doc_serialized(doc: &ParsedDoc, dict_bytes: &[u8]) -> Result<Vec<u8>, String> {
    dump_doc_with_compressor(
        doc,
        |comp_type, content| compress_content(comp_type, content, dict_bytes),
        true,
    )
}

pub fn serialized_content_bytes(doc: &ParsedDoc) -> Result<Vec<u8>, String> {
    dump_content(doc)
}

fn dump_doc_with_compressor(
    doc: &ParsedDoc,
    compress: impl Fn(CompressionType, &[u8]) -> Result<Vec<u8>, String>,
    force_recompress: bool,
) -> Result<Vec<u8>, String> {
    let content = dump_content(doc)?;
    if !force_recompress && content == doc.content_bytes {
        return Ok(doc.raw_bytes.clone());
    }

    let compressed = compress(doc.comp_type, &content)?;
    Ok(rebuild_doc_bytes(doc, &compressed))
}

fn dump_content(doc: &ParsedDoc) -> Result<Vec<u8>, String> {
    let mut serializer = Serializer {
        schema: &doc.schema,
        body: Vec::new(),
        registered_offsets: Vec::new(),
    };
    serializer.write_node(&doc.schema.root_type_ref, &doc.root, None)?;

    let count = serializer.registered_offsets.len() as u32;
    let mut out = Vec::new();
    write_7bit_int(&mut out, count);
    for index in 0..count {
        write_7bit_int(&mut out, index);
    }
    out.extend_from_slice(&serializer.body);
    Ok(out)
}

pub(crate) fn compress_content(
    comp_type: CompressionType,
    content: &[u8],
    dict_bytes: &[u8],
) -> Result<Vec<u8>, String> {
    match comp_type {
        CompressionType::None => Ok(content.to_vec()),
        CompressionType::Zstd => {
            let mut encoder = zstd::stream::Encoder::with_dictionary(Vec::new(), 0, dict_bytes)
                .map_err(|e| format!("zstd init: {}", e))?;
            encoder
                .include_contentsize(true)
                .map_err(|e| format!("zstd include_contentsize: {}", e))?;
            encoder
                .set_pledged_src_size(Some(content.len() as u64))
                .map_err(|e| format!("zstd pledged_src_size: {}", e))?;
            encoder
                .write_all(content)
                .map_err(|e| format!("zstd write: {}", e))?;
            encoder.finish().map_err(|e| format!("zstd finish: {}", e))
        }
    }
}

struct Serializer<'a> {
    schema: &'a CerimalSchema,
    body: Vec<u8>,
    registered_offsets: Vec<usize>,
}

impl<'a> Serializer<'a> {
    fn write_node(
        &mut self,
        declared_type: &CerimalTypeReference,
        node: &ContentNode,
        type_args: Option<&[CerimalTypeReference]>,
    ) -> Result<(), String> {
        let resolved = resolve_type_arg(declared_type, type_args);

        if !needs_tag(resolved, self.schema) {
            return self.write_value(resolved, node, type_args);
        }

        match node {
            ContentNode::Null(_) => {
                write_7bit_int(&mut self.body, 0);
                Ok(())
            }
            ContentNode::Backref { index, .. } => {
                write_7bit_int(&mut self.body, (index << 2) | 1);
                Ok(())
            }
            ContentNode::Polymorphic {
                runtime_type,
                inner,
                ..
            } => {
                write_7bit_int(&mut self.body, 3);
                write_type_reference(&mut self.body, runtime_type)?;
                self.write_node(runtime_type, inner, type_args)
            }
            ContentNode::Concrete(cn) => {
                let tag_payload = self.tag_payload(resolved, &cn.value)?;
                let register_bit = if cn.register_in_backrefs { 1 } else { 0 };
                let tag = 2 | (register_bit << 2) | (tag_payload << 3);
                if cn.register_in_backrefs {
                    self.registered_offsets.push(self.body.len());
                }
                write_7bit_int(&mut self.body, tag);
                self.write_value(resolved, node, type_args)
            }
        }
    }

    fn write_value(
        &mut self,
        type_ref: &CerimalTypeReference,
        node: &ContentNode,
        type_args: Option<&[CerimalTypeReference]>,
    ) -> Result<(), String> {
        let actual = match node {
            ContentNode::Concrete(cn) => cn.as_ref(),
            ContentNode::Polymorphic { inner, .. } => match inner.as_ref() {
                ContentNode::Concrete(cn) => cn.as_ref(),
                _ => return Err("polymorphic inner node is not concrete".to_string()),
            },
            _ => {
                return Err(format!(
                    "expected concrete node for {}",
                    fmt_type_ref(type_ref, self.schema)
                ))
            }
        };

        match type_ref {
            CerimalTypeReference::Primitive(wkt) => self.write_primitive(*wkt, actual),
            CerimalTypeReference::Array { rank, element } => {
                let ContentValue::Array { dims, elements } = &actual.value else {
                    return Err("expected array value".to_string());
                };
                if dims.len() != *rank as usize {
                    return Err(format!(
                        "array rank mismatch: expected {}, got {}",
                        rank,
                        dims.len()
                    ));
                }
                for dim in dims.iter().skip(1) {
                    write_7bit_int(&mut self.body, *dim as u32);
                }
                for elem in elements {
                    self.write_node(element, elem, type_args)?;
                }
                Ok(())
            }
            CerimalTypeReference::TypeArgument { index } => {
                let args = type_args.ok_or("TypeArgument without type_args")?;
                let resolved = args
                    .get(*index as usize)
                    .ok_or_else(|| format!("TypeArgument index {} out of range", index))?;
                self.write_value(resolved, node, type_args)
            }
            CerimalTypeReference::Named {
                guid,
                type_args: named_args,
            } => {
                let idx = self.schema.guid_map.get(guid).copied().ok_or_else(|| {
                    format!("unknown type GUID: {}", fmt_type_ref(type_ref, self.schema))
                })?;
                let typedef = &self.schema.type_defs[idx];
                let effective_args = if !named_args.is_empty() {
                    Some(named_args.as_slice())
                } else {
                    type_args
                };

                match &typedef.members {
                    TypeDefMembers::Enum(em) => {
                        let ContentValue::Enum(value) = &actual.value else {
                            return Err("expected enum value".to_string());
                        };
                        write_enum_value(&mut self.body, &em.underlying_type, *value)?;
                        Ok(())
                    }
                    TypeDefMembers::Class(_) | TypeDefMembers::StructLike(_) => {
                        let ContentValue::Composite(fields) = &actual.value else {
                            return Err("expected composite value".to_string());
                        };
                        self.write_record_fields(typedef, fields, effective_args)
                    }
                }
            }
        }
    }

    fn write_record_fields(
        &mut self,
        typedef: &CerimalTypeDefinition,
        fields: &[(String, ContentNode)],
        type_args: Option<&[CerimalTypeReference]>,
    ) -> Result<(), String> {
        if typedef.kind == TypeDefinitionKind::Class {
            if let Some(base_info) = &typedef.base_info {
                if base_info.has_base {
                    if let Some(base_ref) = &base_info.base_type_ref {
                        let resolved_base = resolve_type_arg(base_ref, type_args);
                        if let CerimalTypeReference::Named {
                            guid,
                            type_args: base_type_args,
                        } = resolved_base
                        {
                            let base_idx = self
                                .schema
                                .guid_map
                                .get(guid)
                                .copied()
                                .ok_or_else(|| "unknown base type guid".to_string())?;
                            let resolved_base_args;
                            let base_args = if !base_type_args.is_empty() {
                                resolved_base_args = base_type_args
                                    .iter()
                                    .map(|arg| resolve_type_arg(arg, type_args).clone())
                                    .collect::<Vec<_>>();
                                Some(resolved_base_args.as_slice())
                            } else {
                                type_args
                            };
                            self.write_record_fields(
                                &self.schema.type_defs[base_idx],
                                fields,
                                base_args,
                            )?;
                        }
                    }
                }
            }
        }

        match &typedef.members {
            TypeDefMembers::Class(members) => {
                for (member_idx, member) in members.iter().enumerate() {
                    match member {
                        ClassMember::Field { name, type_ref } => {
                            let child = find_field(fields, name)?;
                            self.write_node(type_ref, child, type_args)?;
                        }
                        ClassMember::List { element_type } => {
                            let child = find_field(fields, &format!("$list{}", member_idx))?;
                            let concrete = expect_concrete(child)?;
                            let ContentValue::Array { elements, .. } = &concrete.value else {
                                return Err("list member is not backed by array".to_string());
                            };
                            write_7bit_int(&mut self.body, elements.len() as u32);
                            for elem in elements {
                                self.write_node(element_type, elem, type_args)?;
                            }
                        }
                        ClassMember::Dict {
                            key_type,
                            value_type,
                        } => {
                            let child = find_field(fields, &format!("$dict{}", member_idx))?;
                            let concrete = expect_concrete(child)?;
                            let ContentValue::Composite(pairs) = &concrete.value else {
                                return Err(
                                    "dict member is not backed by composite pairs".to_string()
                                );
                            };
                            if pairs.len() % 2 != 0 {
                                return Err("dict member has odd key/value pair count".to_string());
                            }
                            write_7bit_int(&mut self.body, (pairs.len() / 2) as u32);
                            for pair in pairs.chunks_exact(2) {
                                self.write_node(key_type, &pair[0].1, type_args)?;
                                self.write_node(value_type, &pair[1].1, type_args)?;
                            }
                        }
                    }
                }
                Ok(())
            }
            TypeDefMembers::StructLike(members) => {
                if typedef.kind == TypeDefinitionKind::Unmanaged
                    || typedef.kind == TypeDefinitionKind::Struct
                {
                    if compute_blittable_layout(
                        &CerimalTypeReference::Named {
                            guid: typedef.guid,
                            type_args: vec![],
                        },
                        self.schema,
                        type_args,
                    )
                    .is_ok()
                    {
                        self.write_blittable_struct_fields(members, fields, type_args)
                    } else {
                        for member in members {
                            let child = find_field(fields, &member.name)?;
                            self.write_node(&member.type_ref, child, type_args)?;
                        }
                        Ok(())
                    }
                } else {
                    for member in members {
                        let child = find_field(fields, &member.name)?;
                        self.write_node(&member.type_ref, child, type_args)?;
                    }
                    Ok(())
                }
            }
            TypeDefMembers::Enum(_) => Err("write_record_fields on enum typedef".to_string()),
        }
    }

    fn write_blittable_struct_fields(
        &mut self,
        members: &[crate::parser::schema::StructMember],
        fields: &[(String, ContentNode)],
        type_args: Option<&[CerimalTypeReference]>,
    ) -> Result<(), String> {
        let mut blob = Vec::new();
        let mut offset = 0usize;
        let mut max_align = 1usize;

        for member in members {
            let child = find_field(fields, &member.name)?;
            let child_type = resolve_type_arg(&member.type_ref, type_args);
            let (size, align) = compute_blittable_layout(child_type, self.schema, type_args)?;
            let aligned = align_up(offset, align);
            if aligned > blob.len() {
                blob.resize(aligned, 0);
            }
            let bytes = encode_value_to_bytes(self.schema, child_type, child, type_args)?;
            if bytes.len() != size {
                return Err(format!(
                    "blittable field '{}' encoded size mismatch",
                    member.name
                ));
            }
            blob.extend_from_slice(&bytes);
            offset = aligned + size;
            max_align = max_align.max(align);
        }
        let total = align_up(offset, max_align);
        if blob.len() < total {
            blob.resize(total, 0);
        }
        self.body.extend_from_slice(&blob);
        Ok(())
    }

    fn write_primitive(&mut self, wkt: WellKnownType, node: &ConcreteNode) -> Result<(), String> {
        let ContentValue::Primitive(value) = &node.value else {
            return Err(format!("expected primitive value for {}", wkt.name()));
        };
        encode_primitive_value(&mut self.body, wkt, value)
    }

    fn tag_payload(
        &self,
        type_ref: &CerimalTypeReference,
        value: &ContentValue,
    ) -> Result<u32, String> {
        match type_ref {
            CerimalTypeReference::Primitive(WellKnownType::String) => {
                let ContentValue::Primitive(PrimitiveValue::Str(s)) = value else {
                    return Err("string node does not contain string primitive".to_string());
                };
                Ok(s.len() as u32)
            }
            CerimalTypeReference::Array { .. } => {
                let ContentValue::Array { dims, .. } = value else {
                    return Err("array node does not contain array value".to_string());
                };
                Ok(dims.first().copied().unwrap_or(0) as u32)
            }
            _ => Ok(0),
        }
    }
}

fn find_field<'a>(
    fields: &'a [(String, ContentNode)],
    name: &str,
) -> Result<&'a ContentNode, String> {
    fields
        .iter()
        .find(|(field_name, _)| field_name == name)
        .map(|(_, node)| node)
        .ok_or_else(|| format!("field '{}' not found during dump", name))
}

fn expect_concrete(node: &ContentNode) -> Result<&ConcreteNode, String> {
    match node {
        ContentNode::Concrete(cn) => Ok(cn),
        ContentNode::Polymorphic { inner, .. } => expect_concrete(inner),
        _ => Err("expected concrete node".to_string()),
    }
}

fn encode_value_to_bytes(
    schema: &CerimalSchema,
    type_ref: &CerimalTypeReference,
    node: &ContentNode,
    type_args: Option<&[CerimalTypeReference]>,
) -> Result<Vec<u8>, String> {
    let resolved = resolve_type_arg(type_ref, type_args);
    let actual = expect_concrete(node)?;
    match resolved {
        CerimalTypeReference::Primitive(wkt) => {
            let ContentValue::Primitive(value) = &actual.value else {
                return Err("expected primitive value".to_string());
            };
            let mut out = Vec::new();
            encode_primitive_value(&mut out, *wkt, value)?;
            Ok(out)
        }
        CerimalTypeReference::TypeArgument { index } => {
            let args = type_args.ok_or("TypeArgument without type_args")?;
            let resolved = args
                .get(*index as usize)
                .ok_or_else(|| format!("TypeArgument index {} out of range", index))?;
            encode_value_to_bytes(schema, resolved, node, type_args)
        }
        CerimalTypeReference::Named {
            guid,
            type_args: named_args,
        } => {
            let idx = schema
                .guid_map
                .get(guid)
                .copied()
                .ok_or_else(|| "unknown named type".to_string())?;
            let typedef = &schema.type_defs[idx];
            let effective_args = if !named_args.is_empty() {
                Some(named_args.as_slice())
            } else {
                type_args
            };
            match &typedef.members {
                TypeDefMembers::Enum(em) => {
                    let ContentValue::Enum(value) = &actual.value else {
                        return Err("expected enum value".to_string());
                    };
                    let mut out = Vec::new();
                    write_enum_value(&mut out, &em.underlying_type, *value)?;
                    Ok(out)
                }
                TypeDefMembers::StructLike(members) => {
                    let ContentValue::Composite(fields) = &actual.value else {
                        return Err("expected struct composite".to_string());
                    };
                    let mut out = Vec::new();
                    let mut offset = 0usize;
                    let mut max_align = 1usize;
                    for member in members {
                        let child = find_field(fields, &member.name)?;
                        let resolved_member = resolve_type_arg(&member.type_ref, effective_args);
                        let (size, align) =
                            compute_blittable_layout(resolved_member, schema, effective_args)?;
                        let aligned = align_up(offset, align);
                        if aligned > out.len() {
                            out.resize(aligned, 0);
                        }
                        let bytes =
                            encode_value_to_bytes(schema, resolved_member, child, effective_args)?;
                        if bytes.len() != size {
                            return Err(format!(
                                "blittable field '{}' encoded size mismatch",
                                member.name
                            ));
                        }
                        out.extend_from_slice(&bytes);
                        offset = aligned + size;
                        max_align = max_align.max(align);
                    }
                    let total = align_up(offset, max_align);
                    if out.len() < total {
                        out.resize(total, 0);
                    }
                    Ok(out)
                }
                TypeDefMembers::Class(_) => Err("class type is not blittable".to_string()),
            }
        }
        CerimalTypeReference::Array { .. } => Err("array type is not blittable".to_string()),
    }
}

fn encode_primitive_value(
    out: &mut Vec<u8>,
    wkt: WellKnownType,
    value: &PrimitiveValue,
) -> Result<(), String> {
    match (wkt, value) {
        (WellKnownType::Bool, PrimitiveValue::Bool(v)) => out.push(*v as u8),
        (WellKnownType::SByte, PrimitiveValue::I64(v)) => out.push(*v as i8 as u8),
        (WellKnownType::Byte, PrimitiveValue::U64(v)) => out.push(*v as u8),
        (WellKnownType::Short, PrimitiveValue::I64(v)) => {
            out.extend_from_slice(&(*v as i16).to_le_bytes())
        }
        (WellKnownType::UShort | WellKnownType::Char, PrimitiveValue::U64(v)) => {
            out.extend_from_slice(&(*v as u16).to_le_bytes())
        }
        (WellKnownType::Int, PrimitiveValue::I64(v)) => {
            out.extend_from_slice(&(*v as i32).to_le_bytes())
        }
        (WellKnownType::UInt, PrimitiveValue::U64(v)) => {
            out.extend_from_slice(&(*v as u32).to_le_bytes())
        }
        (WellKnownType::Long | WellKnownType::NInt, PrimitiveValue::I64(v)) => {
            out.extend_from_slice(&v.to_le_bytes())
        }
        (
            WellKnownType::ULong | WellKnownType::NUInt | WellKnownType::AssetGuid,
            PrimitiveValue::U64(v),
        ) => out.extend_from_slice(&v.to_le_bytes()),
        (WellKnownType::Float, PrimitiveValue::F64(v)) => {
            out.extend_from_slice(&(*v as f32).to_le_bytes())
        }
        (WellKnownType::Double, PrimitiveValue::F64(v)) => out.extend_from_slice(&v.to_le_bytes()),
        (WellKnownType::Fp, PrimitiveValue::Fp(v)) => {
            let raw = (*v * 65536.0).round() as i64;
            out.extend_from_slice(&raw.to_le_bytes());
        }
        (WellKnownType::Lfp, PrimitiveValue::Lfp(v)) => {
            let raw = (*v * 65536.0).round() as i32;
            out.extend_from_slice(&raw.to_le_bytes());
        }
        (WellKnownType::String, PrimitiveValue::Str(s)) => out.extend_from_slice(s.as_bytes()),
        (WellKnownType::Guid, PrimitiveValue::Guid(bytes)) => out.extend_from_slice(bytes),
        (WellKnownType::Decimal, PrimitiveValue::Bytes(bytes)) => out.extend_from_slice(bytes),
        _ => {
            return Err(format!(
                "primitive/type mismatch while encoding {}",
                wkt.name()
            ))
        }
    }
    Ok(())
}

fn write_enum_value(out: &mut Vec<u8>, ty: &str, value: i64) -> Result<(), String> {
    match ty {
        "u8" => out.push(value as u8),
        "i8" => out.push(value as i8 as u8),
        "u16" => out.extend_from_slice(&(value as u16).to_le_bytes()),
        "i16" => out.extend_from_slice(&(value as i16).to_le_bytes()),
        "u32" => out.extend_from_slice(&(value as u32).to_le_bytes()),
        "i32" => out.extend_from_slice(&(value as i32).to_le_bytes()),
        "u64" => out.extend_from_slice(&(value as u64).to_le_bytes()),
        "i64" => out.extend_from_slice(&value.to_le_bytes()),
        _ => return Err(format!("unknown enum underlying type: {}", ty)),
    }
    Ok(())
}

fn write_type_reference(out: &mut Vec<u8>, type_ref: &CerimalTypeReference) -> Result<(), String> {
    match type_ref {
        CerimalTypeReference::Primitive(wkt) => {
            let code = primitive_code(*wkt)?;
            out.push(code << 3);
        }
        CerimalTypeReference::Array { rank, element } => {
            out.push(((*rank as u8) << 3) | 1);
            write_type_reference(out, element)?;
        }
        CerimalTypeReference::TypeArgument { index } => {
            out.push(((*index as u8) << 3) | 2);
        }
        CerimalTypeReference::Named { guid, type_args } => {
            out.push(((type_args.len() as u8) << 3) | 3);
            out.extend_from_slice(guid);
            for arg in type_args {
                write_type_reference(out, arg)?;
            }
        }
    }
    Ok(())
}

fn primitive_code(wkt: WellKnownType) -> Result<u8, String> {
    match wkt {
        WellKnownType::Bool => Ok(0),
        WellKnownType::SByte => Ok(1),
        WellKnownType::Byte => Ok(2),
        WellKnownType::Short => Ok(3),
        WellKnownType::UShort => Ok(4),
        WellKnownType::Int => Ok(5),
        WellKnownType::UInt => Ok(6),
        WellKnownType::Long => Ok(7),
        WellKnownType::ULong => Ok(8),
        WellKnownType::NInt => Ok(9),
        WellKnownType::NUInt => Ok(10),
        WellKnownType::Char => Ok(11),
        WellKnownType::Double => Ok(12),
        WellKnownType::Float => Ok(13),
        WellKnownType::Decimal => Ok(14),
        WellKnownType::Guid => Ok(15),
        WellKnownType::String => Ok(16),
        WellKnownType::Fp => Ok(17),
        WellKnownType::AssetGuid => Ok(18),
        WellKnownType::Lfp => Ok(19),
        WellKnownType::Unknown(code) => {
            Err(format!("cannot serialize unknown primitive code {}", code))
        }
    }
}

fn write_7bit_int(out: &mut Vec<u8>, mut value: u32) {
    loop {
        let mut b = (value & 0x7F) as u8;
        value >>= 7;
        if value != 0 {
            b |= 0x80;
        }
        out.push(b);
        if value == 0 {
            break;
        }
    }
}
