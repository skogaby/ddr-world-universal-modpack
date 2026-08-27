//! kbin binary → XML string decoder.
//! Simplified from bemani-buddy: outputs XML string directly without intermediate DOM.

use super::sixbit;
use super::types::{self, KbinType};
use super::KbinError;

/// Decode a kbin binary payload into an XML string.
pub fn decode_to_string(data: &[u8]) -> Result<String, KbinError> {
    let header = Header::parse(data)?;
    let node_buf = &data[8..8 + header.node_len];
    let data_start = 12 + header.node_len;
    let data_len = read_u32_be(&data[data_start - 4..data_start]) as usize;
    let data_buf = &data[data_start..data_start + data_len];

    let mut nodes = NodeReader::new(node_buf, header.compressed, header.encoding_idx);
    let mut values = DataReader::new(data_buf, header.encoding_idx);
    let mut xml = String::with_capacity(4096);
    let mut name_stack: Vec<String> = Vec::new();

    loop {
        let raw = nodes.read_u8()?;
        let actual = raw & !types::ARRAY_FLAG;

        if actual == types::FILE_END {
            break;
        }

        if actual == types::NODE_END {
            if let Some(name) = name_stack.pop() {
                xml.push_str("</");
                xml.push_str(&name);
                xml.push('>');
            }
            continue;
        }

        if actual == types::NODE_START {
            let name = nodes.read_name()?;
            xml.push('<');
            xml.push_str(&name);
            emit_attributes(&mut xml, &mut nodes, &mut values)?;
            xml.push('>');
            name_stack.push(name);
            continue;
        }

        // Value type node
        let ktype = types::type_by_id(actual).ok_or(KbinError::UnknownType(actual))?;
        let is_array = (raw & types::ARRAY_FLAG) != 0
            || ktype.id == types::TYPE_BIN
            || ktype.id == types::TYPE_STR;
        let name = nodes.read_name()?;
        let array_size = if is_array {
            values.read_u32()? as usize
        } else {
            ktype.total_size()
        };

        xml.push('<');
        xml.push_str(&name);
        xml.push_str(" __type=\"");
        xml.push_str(ktype.name);
        xml.push('"');

        if ktype.id == types::TYPE_BIN {
            xml.push_str(" __size=\"");
            xml.push_str(&array_size.to_string());
            xml.push('"');
        } else if is_array && ktype.id != types::TYPE_STR {
            let count = array_size / ktype.total_size();
            xml.push_str(" __count=\"");
            xml.push_str(&count.to_string());
            xml.push('"');
        }

        emit_attributes(&mut xml, &mut nodes, &mut values)?;
        xml.push('>');

        // Value text
        emit_value(&mut xml, ktype, is_array, array_size, &mut values)?;

        name_stack.push(name);
        // Value nodes are followed by NODE_END in the stream
        continue;
    }

    // Close any remaining open tags
    while let Some(name) = name_stack.pop() {
        xml.push_str("</");
        xml.push_str(&name);
        xml.push('>');
    }

    Ok(xml)
}

fn emit_attributes(
    xml: &mut String,
    nodes: &mut NodeReader<'_>,
    values: &mut DataReader<'_>,
) -> Result<(), KbinError> {
    while let Ok(next) = nodes.peek_u8() {
        if (next & !types::ARRAY_FLAG) != types::ATTRIBUTE {
            break;
        }
        nodes.read_u8()?;
        let name = nodes.read_name()?;
        let len = values.read_u32()? as usize;
        let val = values.read_string(len)?;
        xml.push(' ');
        xml.push_str(&name);
        xml.push_str("=\"");
        xml_escape_into(xml, &val);
        xml.push('"');
    }
    Ok(())
}

fn emit_value(
    xml: &mut String,
    ktype: &KbinType,
    _is_array: bool,
    array_size: usize,
    values: &mut DataReader<'_>,
) -> Result<(), KbinError> {
    if ktype.id == types::TYPE_BIN {
        let bytes = values.read_aligned(array_size)?;
        for b in &bytes {
            xml.push_str(&format!("{b:02x}"));
        }
    } else if ktype.id == types::TYPE_STR {
        let text = values.read_string(array_size)?;
        xml_escape_into(xml, &text);
    } else {
        let num_elements = if ktype.total_size() > 0 {
            array_size / ktype.total_size()
        } else {
            0
        };
        let bytes = values.read_bytes(array_size)?;
        for i in 0..num_elements {
            if i > 0 {
                xml.push(' ');
            }
            let start = i * ktype.total_size();
            let end = start + ktype.total_size();
            xml.push_str(&ktype.bytes_to_string(&bytes[start..end])?);
        }
    }
    Ok(())
}

fn xml_escape_into(buf: &mut String, s: &str) {
    for c in s.chars() {
        match c {
            '&' => buf.push_str("&amp;"),
            '<' => buf.push_str("&lt;"),
            '>' => buf.push_str("&gt;"),
            '"' => buf.push_str("&quot;"),
            _ => buf.push(c),
        }
    }
}

// -- Header, NodeReader, DataReader ported from bemani-buddy --

struct Header {
    compressed: bool,
    encoding_idx: usize,
    node_len: usize,
}

impl Header {
    fn parse(data: &[u8]) -> Result<Self, KbinError> {
        if data.len() < 8 {
            return Err(KbinError::UnexpectedEof);
        }
        if data[0] != 0xA0 {
            return Err(KbinError::InvalidHeader("missing magic".into()));
        }
        let compressed = match data[1] {
            sixbit::COMPRESSED => true,
            sixbit::UNCOMPRESSED => false,
            b => {
                return Err(KbinError::InvalidHeader(format!(
                    "bad compress flag: 0x{b:02X}"
                )))
            }
        };
        let enc_byte = data[2];
        if data[3] != !enc_byte {
            return Err(KbinError::InvalidHeader("encoding verify mismatch".into()));
        }
        let encoding_idx = (enc_byte >> 5) as usize;
        let node_len = read_u32_be(&data[4..8]) as usize;
        if data.len() < 12 + node_len {
            return Err(KbinError::UnexpectedEof);
        }
        Ok(Header {
            compressed,
            encoding_idx,
            node_len,
        })
    }
}

struct NodeReader<'a> {
    data: &'a [u8],
    pos: usize,
    compressed: bool,
    encoding_idx: usize,
}

impl<'a> NodeReader<'a> {
    fn new(data: &'a [u8], compressed: bool, encoding_idx: usize) -> Self {
        Self {
            data,
            pos: 0,
            compressed,
            encoding_idx,
        }
    }
    fn read_u8(&mut self) -> Result<u8, KbinError> {
        if self.pos >= self.data.len() {
            return Err(KbinError::UnexpectedEof);
        }
        let b = self.data[self.pos];
        self.pos += 1;
        Ok(b)
    }
    fn peek_u8(&self) -> Result<u8, KbinError> {
        if self.pos >= self.data.len() {
            return Err(KbinError::UnexpectedEof);
        }
        Ok(self.data[self.pos])
    }
    fn read_bytes(&mut self, n: usize) -> Result<&'a [u8], KbinError> {
        if self.pos + n > self.data.len() {
            return Err(KbinError::UnexpectedEof);
        }
        let slice = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(slice)
    }
    fn read_name(&mut self) -> Result<String, KbinError> {
        let length = self.read_u8()? as usize;
        if self.compressed {
            let byte_count = sixbit::encoded_length(length);
            let bytes = self.read_bytes(byte_count)?;
            sixbit::decode(bytes, length)
        } else {
            let byte_count = (length & !64) + 1;
            let bytes = self.read_bytes(byte_count)?;
            decode_text(bytes, self.encoding_idx)
        }
    }
}

struct DataReader<'a> {
    data: &'a [u8],
    pos8: usize,
    pos16: usize,
    pos32: usize,
    encoding_idx: usize,
}

impl<'a> DataReader<'a> {
    fn new(data: &'a [u8], encoding_idx: usize) -> Self {
        Self {
            data,
            pos8: 0,
            pos16: 0,
            pos32: 0,
            encoding_idx,
        }
    }
    fn read_bytes(&mut self, n: usize) -> Result<Vec<u8>, KbinError> {
        if n == 0 {
            return Ok(Vec::new());
        }
        let result = match n {
            1 => {
                self.check(self.pos8, 1)?;
                vec![self.data[self.pos8]]
            }
            2 => {
                self.check(self.pos16, 2)?;
                self.data[self.pos16..self.pos16 + 2].to_vec()
            }
            _ => {
                self.check(self.pos32, n)?;
                self.data[self.pos32..self.pos32 + n].to_vec()
            }
        };
        self.realign(n);
        Ok(result)
    }
    fn read_aligned(&mut self, n: usize) -> Result<Vec<u8>, KbinError> {
        if n == 0 {
            return Ok(Vec::new());
        }
        self.check(self.pos32, n)?;
        let result = self.data[self.pos32..self.pos32 + n].to_vec();
        let padded = if !n.is_multiple_of(4) {
            n + (4 - n % 4)
        } else {
            n
        };
        self.realign(padded);
        Ok(result)
    }
    fn read_u32(&mut self) -> Result<u32, KbinError> {
        let bytes = self.read_bytes(4)?;
        Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }
    fn read_string(&mut self, length: usize) -> Result<String, KbinError> {
        let bytes = self.read_aligned(length)?;
        if bytes.is_empty() {
            return Ok(String::new());
        }
        let end = if bytes.last() == Some(&0) {
            bytes.len() - 1
        } else {
            bytes.len()
        };
        decode_text(&bytes[..end], self.encoding_idx)
    }
    fn realign(&mut self, n: usize) {
        match n {
            1 => {
                if self.pos8.is_multiple_of(4) {
                    self.pos32 += 4;
                }
                self.pos8 += 1;
            }
            2 => {
                if self.pos16.is_multiple_of(4) {
                    self.pos32 += 4;
                }
                self.pos16 += 2;
            }
            n => {
                let padded = if n % 4 != 0 { n + (4 - n % 4) } else { n };
                self.pos32 += padded;
            }
        }
        if self.pos8.is_multiple_of(4) {
            self.pos8 = self.pos32;
        }
        if self.pos16.is_multiple_of(4) {
            self.pos16 = self.pos32;
        }
    }
    fn check(&self, pos: usize, n: usize) -> Result<(), KbinError> {
        if pos + n > self.data.len() {
            Err(KbinError::UnexpectedEof)
        } else {
            Ok(())
        }
    }
}

fn read_u32_be(data: &[u8]) -> u32 {
    u32::from_be_bytes([data[0], data[1], data[2], data[3]])
}

fn decode_text(bytes: &[u8], _encoding_idx: usize) -> Result<String, KbinError> {
    if bytes.is_empty() {
        return Ok(String::new());
    }
    // For layeredfs we only need ASCII/UTF-8; lossy fallback for others
    Ok(String::from_utf8_lossy(bytes).into_owned())
}
