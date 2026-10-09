//! A minimal ZIP writer for EPUB containers: stored and deflated entries in the order added, with fixed
//! timestamps and no extra fields, so the same entries give the same bytes.

use std::io::Write;

use flate2::Crc;
use flate2::write::DeflateEncoder;

const LOCAL_HEADER: u32 = 0x0403_4b50;
const CENTRAL_HEADER: u32 = 0x0201_4b50;
const END_OF_CENTRAL_DIRECTORY: u32 = 0x0605_4b50;
const STORED: u16 = 0;
const DEFLATED: u16 = 8;
/// 1980-01-01 00:00, the earliest DOS date.
const DOS_DATE: u16 = (1 << 5) | 1;
const DOS_TIME: u16 = 0;

#[derive(Default)]
pub struct Zip {
    data: Vec<u8>,
    central: Vec<u8>,
    entries: u16,
}

impl Zip {
    /// Adds a file. A compressed file is stored as is where deflating does not make it smaller.
    pub fn add(&mut self, name: &str, content: &[u8], compress: bool) -> Result<(), String> {
        let deflated = compress
            .then(|| deflate(content))
            .filter(|packed| packed.len() < content.len());
        let (method, packed) = match &deflated {
            Some(packed) => (DEFLATED, packed.as_slice()),
            None => (STORED, content),
        };
        let mut crc = Crc::new();
        crc.update(content);
        let too_large = || format!("{name} is too large for a ZIP file without ZIP64");
        let size = u32::try_from(content.len()).map_err(|_| too_large())?;
        let packed_size = u32::try_from(packed.len()).map_err(|_| too_large())?;
        let offset = u32::try_from(self.data.len()).map_err(|_| too_large())?;
        let name_length = u16::try_from(name.len()).map_err(|_| format!("{name} is too long a ZIP entry name"))?;
        self.entries = self
            .entries
            .checked_add(1)
            .ok_or("too many files for a ZIP file without ZIP64")?;
        let version: u16 = if method == DEFLATED { 20 } else { 10 };
        // Version needed, flags, method, time, date, CRC, sizes, and name length, shared by both headers.
        let mut common = Vec::with_capacity(24);
        for value in [version, 0, method, DOS_TIME, DOS_DATE] {
            common.extend_from_slice(&value.to_le_bytes());
        }
        for value in [crc.sum(), packed_size, size] {
            common.extend_from_slice(&value.to_le_bytes());
        }
        common.extend_from_slice(&name_length.to_le_bytes());

        self.data.extend_from_slice(&LOCAL_HEADER.to_le_bytes());
        self.data.extend_from_slice(&common);
        self.data.extend_from_slice(&0u16.to_le_bytes());
        self.data.extend_from_slice(name.as_bytes());
        self.data.extend_from_slice(packed);

        self.central.extend_from_slice(&CENTRAL_HEADER.to_le_bytes());
        self.central.extend_from_slice(&version.to_le_bytes());
        self.central.extend_from_slice(&common);
        // Extra field and comment lengths, disk, internal and external attributes.
        self.central.extend_from_slice(&[0; 2 + 2 + 2 + 2 + 4]);
        self.central.extend_from_slice(&offset.to_le_bytes());
        self.central.extend_from_slice(name.as_bytes());
        Ok(())
    }

    /// The archive with its central directory.
    pub fn finish(mut self) -> Result<Vec<u8>, String> {
        let too_large = || "the EPUB is too large for a ZIP file without ZIP64".to_owned();
        let offset = u32::try_from(self.data.len()).map_err(|_| too_large())?;
        let size = u32::try_from(self.central.len()).map_err(|_| too_large())?;
        self.data.append(&mut self.central);
        self.data.extend_from_slice(&END_OF_CENTRAL_DIRECTORY.to_le_bytes());
        self.data.extend_from_slice(&[0; 4]);
        self.data.extend_from_slice(&self.entries.to_le_bytes());
        self.data.extend_from_slice(&self.entries.to_le_bytes());
        self.data.extend_from_slice(&size.to_le_bytes());
        self.data.extend_from_slice(&offset.to_le_bytes());
        self.data.extend_from_slice(&[0; 2]);
        Ok(self.data)
    }
}

fn deflate(content: &[u8]) -> Vec<u8> {
    let mut encoder = DeflateEncoder::new(Vec::new(), flate2::Compression::best());
    encoder.write_all(content).expect("writing to memory succeeds");
    encoder.finish().expect("writing to memory succeeds")
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use super::*;

    fn u16_at(data: &[u8], at: usize) -> u16 {
        u16::from_le_bytes([data[at], data[at + 1]])
    }

    fn u32_at(data: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(data[at..at + 4].try_into().unwrap())
    }

    #[test]
    fn stores_the_first_entry_uncompressed_and_deflates_text_readably() {
        let text = "the lamp turned on a bath of mercury. ".repeat(40);
        let mut zip = Zip::default();
        zip.add("mimetype", b"application/epub+zip", false).unwrap();
        zip.add("EPUB/text.xhtml", text.as_bytes(), true).unwrap();
        let data = zip.finish().unwrap();

        assert_eq!(u32_at(&data, 0), LOCAL_HEADER);
        assert_eq!(u16_at(&data, 8), STORED);
        assert_eq!(&data[30..38], b"mimetype");
        assert_eq!(&data[38..58], b"application/epub+zip");

        let second = 58;
        assert_eq!(u32_at(&data, second), LOCAL_HEADER);
        assert_eq!(u16_at(&data, second + 8), DEFLATED);
        let packed = u32_at(&data, second + 18) as usize;
        let start = second + 30 + u16_at(&data, second + 26) as usize;
        let mut inflated = String::new();
        flate2::read::DeflateDecoder::new(&data[start..start + packed])
            .read_to_string(&mut inflated)
            .unwrap();
        assert_eq!(inflated, text);

        let end = data.len() - 22;
        assert_eq!(u32_at(&data, end), END_OF_CENTRAL_DIRECTORY);
        assert_eq!(u16_at(&data, end + 10), 2);
        assert_eq!(u32_at(&data, u32_at(&data, end + 16) as usize), CENTRAL_HEADER);
    }
}
