# CERIMAL Binary Format — Specification

## File Layout

```
Offset  Size  Description
------  ----  -----------
0x00    7     Magic: "CERIMAL" (ASCII)
0x07    1     Version: 0x03 (hardcoded in current builds)
0x08    4     schema_size (u32 LE): byte length of schema section
0x0C    4     content_size (u32 LE): byte length of content (compressed size if compressed)
0x10    4     compression_type (u32 LE): 0=None, 1=ZstdSaveData
0x14    8     xxHash64 checksum of all bytes after this header (schema + backref + content)
------  ----  (28 bytes total)
0x1C    ?     Schema section
?       ?     Back-reference mapping table
?       ?     Content section (possibly Zstd-compressed)
```

- Only content is compressed; header and schema are always raw.
- Falls back to `CompressionType=None` if content ≤256 bytes or compression fails.

---

## Schema Section

Layout (in order):

1. **16-byte header** (written last, sizes filled in once both loops complete):

```
+0x00  4  global_version (u32)
+0x04  4  type_defs_total_bytes: total byte length of all type def records
+0x08  4  type_def_count: number of type definitions
+0x0C  4  name_strings_total_bytes: total byte length of name strings pool
```

2. **Type definition records** — one per type, sorted by GUID ascending.
3. **Name strings pool** — one UTF-8 name per type, same GUID-sorted order, positionally aligned (record N → name N).
4. **Root type reference** — TypeRef of the document root node.

### Type Definition Record

Fixed 28-byte header:

```
+0x00  4   total_record_bytes
+0x04  4   flags_packed (u32):
             bits[1:0]  = kind - 1  (0=Reference/class, 1=Value/struct, 2=Unmanaged, 3=Enum)
             bit[2]     = has_base_or_interfaces
             bits[7:3]  = type_parameter_count
             bits[31:8] = member_count
+0x08  4   local_version (u32)
+0x0C  16  GUID (16 bytes)
```

If `has_base_or_interfaces`:

```
4  base_list_header (u32):
     bits[23:0]  = base_list_byte_size
     bit[24]     = has_base_type
     bits[31:25] = interface_count
[optional] base TypeRef
[optional] N × interface TypeRef
```

Members (×member_count) — encoding differs by kind:

**Kind=1 (class)** — each member has a `u8` kind prefix:
```
[u8]  member_kind - 1:
        0 = List   → [element TypeRef]
        1 = Dict   → [key TypeRef] [value TypeRef]
        2 = Simple → [1-byte-len string: field name] [TypeRef]
```

**Kind=2/3 (struct/unmanaged)** — NO kind prefix byte:
```
[1-byte-len string: field name] [TypeRef]
```

> Critical: assuming a kind byte for structs will desync the parser.

**Kind=4 (enum)** — after 28-byte header:
```
[1 byte]  m_packed: bits[2:0] = underlying type ID
            Even IDs = unsigned: 0=u8, 2=u16, 4=u32, 6=u64
            Odd IDs  = signed:   1=i8, 3=i16, 5=i32, 7=i64
            Value byte size = 1 << (id >> 1)
Then ×member_count:
  [1-byte-len string]  variant name
  [1/2/4/8 bytes LE]   variant value
```

Name strings pool: each name is a **7-bit-length-prefixed** UTF-8 string.
Member names: **1-byte-length-prefixed** UTF-8 string (max 255 chars).

---

## Back-Reference Mapping Table

Between schema and content. Allows shared objects to appear once in content and be referenced by index (DAG, not pure tree).

```
[7-bit int]  count
[7-bit int]  content_offset[0]   ← byte offset into content of back-ref object 0
...
[7-bit int]  content_offset[N-1]
```

Offsets are sorted ascending. If no back-references: just `0x00`.

---

## Content Section

Recursive node tree. Root node type is given by the root TypeRef in the schema.

### Node Tag

Each node starts with a **7-bit encoded integer** tag:

```
bits[1:0] = node_kind:
  0x0 = Null
  0x1 = Back-reference  → bits[31:2] = back-ref index into mapping table
  0x2 = Concrete        → type known from schema context
  0x3 = Polymorphic     → followed by inline TypeRef, then a FULL inner node
bit[2]    = register as back-ref target (only meaningful for kind=2 and kind=3)
bits[31:3] = payload (only meaningful for kind=2 and kind=3; see below)
```

> **Back-reference note:** For kind=1, `bits[31:2]` is the full back-ref index — bit[2] is part of the index, not the "register as target" flag. The register flag only applies to kind=2 and kind=3.

Special payload encoding (for kind=2 and kind=3):
- **String** (WellKnownType=16): `bits[31:3]` = UTF-8 byte length → immediately followed by that many bytes.
- **Array** (TypeRef Kind=1): `bits[31:3]` = first dimension size → additional 7-bit ints for extra dimensions.
- **Composite — class or value struct** (TypeDef kind=1/2): no payload — delegates to RecordList (see below).
- **Composite — unmanaged struct** (TypeDef kind=3): no payload — delegates to raw byte blob (see Unmanaged Type Fields below).

> The node tag kind (bits[1:0]) and the TypeDef kind are independent. Both struct variants produce a kind=2 concrete node, but what follows in the stream differs based on the schema's TypeDef kind.

#### Polymorphic node layout

A polymorphic node does **not** immediately precede value data. The stream layout is:

```
[poly_tag: kind=3]  [runtime TypeRef bytes]  [inner_tag: kind=2, payload=P]  [content...]
```

After reading the runtime TypeRef, there is a **second, complete node tag** (the inner concrete node) with its own payload. The payload `P` from this inner tag drives string/array parsing. This is because `DeserializeCore` is structured as a `while(1)` loop: it reads a tag, and for polymorphic nodes re-reads the type and loops back to read another tag before breaking on `kind=2`. A parser must read the inner tag as a full node, not skip directly to content after the TypeRef.

### RecordList (Class and Value Struct Fields)

Applies to TypeDef kind=1 (class) and kind=2 (value struct). Fields are serialized in declaration order. If the type has a base class, base fields come first (recursively). Each field is a full CERIMAL node (tag + value).

| Schema member kind | Content encoding |
|--------------------|-----------------|
| Simple (3) | one node of the field's type |
| List (1) | 7-bit int count, then N nodes of element type |
| Dict (2) | 7-bit int count, then N × (key node, value node) |

### Unmanaged Type Fields

Applies to TypeDef kind=3 (unmanaged struct). In CERIMAL, **UNMANAGED** means the type is serialized **inline without a node tag** (no null/backref/polymorphic overhead). This is distinct from whether the type is a raw byte blob or not.

Two sub-cases:

**Case A — truly blittable:** all fields recursively resolve to primitives, enums, or other blittable unmanaged structs (no arrays, strings, or class references). CERIMAL serializes this as a **raw byte blob** with no per-field tags.

- **Stream layout:** No per-field tags, no length prefix. The blob size is implicit — computed from the schema.
- **Layout algorithm** (mirrors `ComputeLayoutForUnmanagedTypeDescription`):

```
offset = 0
max_align = 1

for each field in schema member order:
    a = field_align   # for primitives: same as field_size
    offset = ceil(offset, a)   # advance to next multiple of a
    field_offset[i] = offset
    offset += field_size
    max_align = max(max_align, a)

total_size = ceil(offset, max_align)   # pad end of struct to its own alignment
```

For nested unmanaged structs, recurse to get their `total_size` and `max_align`. For primitives, `field_size = field_align = WellKnownType.size`.

**Case B — non-blittable fields:** the schema lists the type as UNMANAGED but its members transitively include arrays, classes, or structs with managed fields (e.g. `PersistentItemInstance` which contains `SerializedNullable<PersistentEnchantmentData>`, which in turn has `Enchantment[]` and `GemEnchantment[]` arrays). In this case, CERIMAL falls back to **field-by-field serialization** — same as STRUCT, but still without a leading node tag.

> The distinction between Case A and Case B must be discovered dynamically by attempting to compute the blittable layout and falling back if any field is non-blittable. The `CerimalTypeReferenceArray.rank` field is **array dimensionality** (1 = `T[]`, 2 = `T[,]`), not an element count — a `T[]` array with rank=1 is always a dynamic reference type and never blittable inline.

### Primitive Encoding (WellKnownType)

| ID | C# type | Size | Encoding |
|----|---------|------|----------|
| 0 | bool | 1 | 1 byte |
| 1 | sbyte | 1 | 1 byte signed |
| 2 | byte | 1 | 1 byte unsigned |
| 3 | short | 2 | LE |
| 4 | ushort | 2 | LE |
| 5 | int | 4 | LE |
| 6 | uint | 4 | LE |
| 7 | long | 8 | LE |
| 8 | ulong | 8 | LE |
| 9 | nint | 8 | LE |
| 10 | nuint | 8 | LE |
| 11 | char | 2 | LE |
| 12 | double | 8 | LE |
| 13 | float | 4 | LE |
| 14 | decimal | 16 | raw (align=4) |
| 15 | Guid | 16 | raw (align=4) |
| 16 | string | — | length in node tag; not blittable |
| 17 | FP (fixed-point) | 8 | LE i64, real = raw / 65536 |
| 18 | AssetGuid | 8 | LE |
| 19 | LFP (light fixed-point) | 4 | LE i32, real = raw / 65536 |

All values are raw little-endian — never 7-bit encoded. `decimal` and `Guid` are the only types where alignment (4) differs from size (16); all others have `align = size`.

> String (ID=16) is not blittable and cannot appear inside an unmanaged struct.

---

## TypeReference Encoding

1-byte header, `bits[1:0]` = kind, `bits[7:3]` = payload (bit 2 is unused/zero):

| Kind | Header | Trailing bytes |
|------|--------|---------------|
| 0 — Primitive | `bits[7:3]` = WellKnownType ID | none |
| 1 — Array | `bits[7:3]` = rank | element TypeRef (recursive) |
| 2 — TypeArgument | `bits[7:3]` = generic param index | none |
| 3 — Named type | `bits[7:3]` = type_arg_count | 16-byte GUID + N×TypeRef (recursive) |

> **Bug note (bits[7:2] vs bits[7:3]):** The original spec incorrectly documented
> Array and TypeArgument as using `bits[7:2]` (6 bits, i.e. `header >> 2`) for
> their payload. The actual game code (`TypeReference.Raw.GetArray` and
> `STypeReference` constructor) uses `bits[7:3]` (5 bits, i.e. `header >> 3`)
> for ALL four kinds — bit 2 is not part of the payload for any kind.
>
> **Why this matters:** A `GUID[]` array (rank 1) encodes as byte `0x09`:
> `(1 << 3) | 1 = 0x09`. With the wrong shift (`0x09 >> 2 = 2`), the parser
> reads rank=2 and expects two dimension sizes from the content stream — the
> first from the node tag payload, the second as an extra 7-bit int. With the
> correct shift (`0x09 >> 3 = 1`), rank=1 and only the tag payload dimension
> is used. The phantom "second dimension" byte consumed real data, causing every
> field after the first array to be misaligned. In practice this shifted the
> entire content stream by ~35 bytes, producing garbage for all fields past the
> first array in the save file.

Examples:
- `int` → `(5 << 3) | 0 = 0x28`
- `int[]` (rank 1) → `(1 << 3) | 1 = 0x09` + `0x28`
- `Item` (non-generic) → `(0 << 3) | 3 = 0x03` + GUID
- `List<int>` → `(1 << 3) | 3 = 0x0B` + GUID of List + `0x28`

---

## 7-Bit Integer Encoding

LEB128-style, little-endian:
- Each byte contributes 7 bits (low bits first).
- High bit set = more bytes follow.
- Max 5 bytes (32-bit values).

Used for: node tags, list counts, backref table entries, string lengths in name pool.
NOT used for: primitive field values (those are raw LE).
