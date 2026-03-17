pub struct Reader<'a> {
    pub data: &'a [u8],
    pub pos: usize,
}

impl<'a> Reader<'a> {
    pub fn from_slice(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    pub fn read_slice(&mut self, n: usize) -> Result<&'a [u8], String> {
        if self.pos + n > self.data.len() {
            return Err(format!(
                "read {} bytes at pos {}: out of bounds (len {})",
                n,
                self.pos,
                self.data.len()
            ));
        }
        let chunk = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(chunk)
    }

    pub fn read_bytes(&mut self, n: usize) -> Result<Vec<u8>, String> {
        Ok(self.read_slice(n)?.to_vec())
    }

    pub fn read_array<const N: usize>(&mut self) -> Result<[u8; N], String> {
        self.read_slice(N)?
            .try_into()
            .map_err(|_| format!("failed to read {} bytes", N))
    }

    pub fn u8(&mut self) -> Result<u8, String> {
        if self.pos >= self.data.len() {
            return Err(format!(
                "read u8 at pos {}: out of bounds (len {})",
                self.pos,
                self.data.len()
            ));
        }
        let b = self.data[self.pos];
        self.pos += 1;
        Ok(b)
    }

    pub fn i16_le(&mut self) -> Result<i16, String> {
        Ok(i16::from_le_bytes(self.read_array()?))
    }

    pub fn u16_le(&mut self) -> Result<u16, String> {
        Ok(u16::from_le_bytes(self.read_array()?))
    }

    pub fn i32_le(&mut self) -> Result<i32, String> {
        Ok(i32::from_le_bytes(self.read_array()?))
    }

    pub fn u32_le(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.read_array()?))
    }

    pub fn i64_le(&mut self) -> Result<i64, String> {
        Ok(i64::from_le_bytes(self.read_array()?))
    }

    pub fn u64_le(&mut self) -> Result<u64, String> {
        Ok(u64::from_le_bytes(self.read_array()?))
    }

    pub fn f32_le(&mut self) -> Result<f32, String> {
        Ok(f32::from_le_bytes(self.read_array()?))
    }

    pub fn f64_le(&mut self) -> Result<f64, String> {
        Ok(f64::from_le_bytes(self.read_array()?))
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
        let bytes = self.read_slice(len)?;
        std::str::from_utf8(bytes)
            .map(|s| s.to_owned())
            .map_err(|e| e.to_string())
    }

    /// Read a name with 7-bit encoded length prefix.
    pub fn read_7bit_name(&mut self) -> Result<String, String> {
        let len = self.read_7bit_int()? as usize;
        let bytes = self.read_slice(len)?;
        std::str::from_utf8(bytes)
            .map(|s| s.to_owned())
            .map_err(|e| e.to_string())
    }

    /// Read an integer value using an enum underlying type string ("u8", "i32", etc.).
    pub fn unpack_enum_type(&mut self, ty: &str) -> Result<i64, String> {
        match ty {
            "u8" => Ok(self.u8()? as i64),
            "i8" => Ok(self.u8()? as i8 as i64),
            "u16" => Ok(self.u16_le()? as i64),
            "i16" => Ok(self.i16_le()? as i64),
            "u32" => Ok(self.u32_le()? as i64),
            "i32" => Ok(self.i32_le()? as i64),
            "u64" => Ok(self.u64_le()? as i64),
            "i64" => Ok(self.i64_le()?),
            _ => Err(format!("Unknown enum underlying type: {}", ty)),
        }
    }
}
