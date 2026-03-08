pub struct Reader {
    pub data: Vec<u8>,
    pub pos: usize,
}

impl Reader {
    pub fn new(data: Vec<u8>) -> Self {
        Self { data, pos: 0 }
    }

    pub fn from_slice(data: &[u8]) -> Self {
        Self { data: data.to_vec(), pos: 0 }
    }

    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    pub fn read_bytes(&mut self, n: usize) -> Result<Vec<u8>, String> {
        if self.pos + n > self.data.len() {
            return Err(format!(
                "read {} bytes at pos {}: out of bounds (len {})",
                n, self.pos, self.data.len()
            ));
        }
        let chunk = self.data[self.pos..self.pos + n].to_vec();
        self.pos += n;
        Ok(chunk)
    }

    pub fn u8(&mut self) -> Result<u8, String> {
        if self.pos >= self.data.len() {
            return Err(format!("read u8 at pos {}: out of bounds (len {})", self.pos, self.data.len()));
        }
        let b = self.data[self.pos];
        self.pos += 1;
        Ok(b)
    }

    pub fn u32_le(&mut self) -> Result<u32, String> {
        let b = self.read_bytes(4)?;
        Ok(u32::from_le_bytes(b.try_into().unwrap()))
    }

    pub fn u64_le(&mut self) -> Result<u64, String> {
        let b = self.read_bytes(8)?;
        Ok(u64::from_le_bytes(b.try_into().unwrap()))
    }

    pub fn read_7bit_int(&mut self) -> Result<u32, String> {
        let mut result: u32 = 0;
        let mut shift = 0u32;
        loop {
            let b = self.u8()?;
            result |= ((b & 0x7F) as u32) << shift;
            if b & 0x80 == 0 {
                break;
            }
            shift += 7;
        }
        Ok(result)
    }

    /// Read a name with u8 length prefix.
    pub fn read_name(&mut self) -> Result<String, String> {
        let len = self.u8()? as usize;
        let bytes = self.read_bytes(len)?;
        String::from_utf8(bytes).map_err(|e| e.to_string())
    }

    /// Read a name with 7-bit encoded length prefix.
    pub fn read_7bit_name(&mut self) -> Result<String, String> {
        let len = self.read_7bit_int()? as usize;
        let bytes = self.read_bytes(len)?;
        String::from_utf8(bytes).map_err(|e| e.to_string())
    }

    /// Read an integer value using an enum underlying type string ("u8", "i32", etc.).
    pub fn unpack_enum_type(&mut self, ty: &str) -> Result<i64, String> {
        match ty {
            "u8" => Ok(self.u8()? as i64),
            "i8" => Ok(self.u8()? as i8 as i64),
            "u16" => {
                let b = self.read_bytes(2)?;
                Ok(u16::from_le_bytes(b.try_into().unwrap()) as i64)
            }
            "i16" => {
                let b = self.read_bytes(2)?;
                Ok(i16::from_le_bytes(b.try_into().unwrap()) as i64)
            }
            "u32" => {
                let b = self.read_bytes(4)?;
                Ok(u32::from_le_bytes(b.try_into().unwrap()) as i64)
            }
            "i32" => {
                let b = self.read_bytes(4)?;
                Ok(i32::from_le_bytes(b.try_into().unwrap()) as i64)
            }
            "u64" => {
                let b = self.read_bytes(8)?;
                Ok(u64::from_le_bytes(b.try_into().unwrap()) as i64)
            }
            "i64" => {
                let b = self.read_bytes(8)?;
                Ok(i64::from_le_bytes(b.try_into().unwrap()))
            }
            _ => Err(format!("Unknown enum underlying type: {}", ty)),
        }
    }
}
