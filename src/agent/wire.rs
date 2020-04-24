use std::io::{Read, Write};

use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use openssl::bn::{BigNum, BigNumRef};
use wrapperrs::{Result, ResultExt};

pub trait ReadExt: Read {
    fn read_string(&mut self) -> Result<Vec<u8>> {
        let len = self.read_u32::<BigEndian>().wrap_err("read len")?;
        let mut content = vec![0; len as usize];
        self.read_exact(&mut content).wrap_err("read content")?;
        Ok(content)
    }

    fn read_string_utf8(&mut self) -> Result<String> {
        Ok(String::from_utf8(self.read_string()?).wrap_err("decode string")?)
    }

    fn read_mpint(&mut self) -> Result<BigNum> {
        Ok(BigNum::from_slice(&self.read_string()?).unwrap())
    }
}

impl<T: Read> ReadExt for T {}

pub trait WriteExt: Write {
    fn write_string<T: AsRef<[u8]>>(&mut self, data: T) -> std::io::Result<()> {
        let data: &[u8] = data.as_ref();
        self.write_u32::<BigEndian>(data.len() as u32)?;
        self.write_all(data)
    }

    fn write_mpint(&mut self, bn: &BigNumRef) -> std::io::Result<()> {
        let mut bytes = bn.to_vec();
        if bytes[0] & 0x80 > 0 {
            bytes.insert(0, 0);
        }
        self.write_string(bytes)
    }
}

impl<T: Write> WriteExt for T {}
