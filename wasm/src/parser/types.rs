/// WellKnownType mirrors the Python WellKnownType enum, including size/alignment info.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WellKnownType {
    Bool,
    SByte,
    Byte,
    Short,
    UShort,
    Int,
    UInt,
    Long,
    ULong,
    NInt,
    NUInt,
    Char,
    Double,
    Float,
    Decimal,
    Guid,
    String,
    Fp,
    AssetGuid,
    Lfp,
    Unknown(u8),
}

impl WellKnownType {
    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => Self::Bool,
            1 => Self::SByte,
            2 => Self::Byte,
            3 => Self::Short,
            4 => Self::UShort,
            5 => Self::Int,
            6 => Self::UInt,
            7 => Self::Long,
            8 => Self::ULong,
            9 => Self::NInt,
            10 => Self::NUInt,
            11 => Self::Char,
            12 => Self::Double,
            13 => Self::Float,
            14 => Self::Decimal,
            15 => Self::Guid,
            16 => Self::String,
            17 => Self::Fp,
            18 => Self::AssetGuid,
            19 => Self::Lfp,
            other => Self::Unknown(other),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Bool => "BOOL",
            Self::SByte => "SBYTE",
            Self::Byte => "BYTE",
            Self::Short => "SHORT",
            Self::UShort => "USHORT",
            Self::Int => "INT",
            Self::UInt => "UINT",
            Self::Long => "LONG",
            Self::ULong => "ULONG",
            Self::NInt => "NINT",
            Self::NUInt => "NUINT",
            Self::Char => "CHAR",
            Self::Double => "DOUBLE",
            Self::Float => "FLOAT",
            Self::Decimal => "DECIMAL",
            Self::Guid => "GUID",
            Self::String => "STRING",
            Self::Fp => "FP",
            Self::AssetGuid => "ASSETGUID",
            Self::Lfp => "LFP",
            Self::Unknown(_) => "UNKNOWN",
        }
    }

    /// Returns (byte_size, alignment) for fixed-size primitives. (0, 0) for STRING.
    pub fn size_align(self) -> (usize, usize) {
        match self {
            Self::Bool | Self::SByte | Self::Byte => (1, 1),
            Self::Short | Self::UShort | Self::Char => (2, 2),
            Self::Int | Self::UInt | Self::Float | Self::Lfp => (4, 4),
            Self::Long | Self::ULong | Self::NInt | Self::NUInt
            | Self::Double | Self::Fp | Self::AssetGuid => (8, 8),
            Self::Decimal | Self::Guid => (16, 4),
            Self::String => (0, 0),
            Self::Unknown(_) => (0, 0),
        }
    }

    pub fn is_reference(self) -> bool {
        matches!(self, Self::String)
    }
}

#[derive(Debug, Clone)]
pub enum CerimalTypeReference {
    Primitive(WellKnownType),
    Array {
        rank: u32,
        element: Box<CerimalTypeReference>,
    },
    TypeArgument {
        index: u32,
    },
    Named {
        guid: [u8; 16],
        type_args: Vec<CerimalTypeReference>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CompressionType {
    None,
    Zstd,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TypeDefinitionKind {
    Class,
    Struct,
    Unmanaged,
    Enum,
}

impl TypeDefinitionKind {
    pub fn from_u8(v: u8) -> Result<Self, String> {
        match v {
            0 => Ok(Self::Class),
            1 => Ok(Self::Struct),
            2 => Ok(Self::Unmanaged),
            3 => Ok(Self::Enum),
            _ => Err(format!("Unknown TypeDefinitionKind: {}", v)),
        }
    }
}
