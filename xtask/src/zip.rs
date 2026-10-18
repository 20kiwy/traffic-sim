//! Minimal ZIP writer (stored entries, no compression) — std only.
//!
//! Good enough for packaging source trees + PNG screenshots, which are small and
//! already compressed. Follows VectorCraft's philosophy: xtask stays pure Rust.

use std::io::Write;
use std::path::{Path, PathBuf};

pub struct ZipWriter {
    buf: Vec<u8>,
    entries: Vec<(String, u64, u32, u32)>, // name, offset, crc, size
}

fn crc32(data: &[u8]) -> u32 {
    static mut TABLE: [u32; 256] = [0; 256];
    static INIT: std::sync::Once = std::sync::Once::new();
    unsafe {
        INIT.call_once(|| {
            for i in 0..256u32 {
                let mut c = i;
                for _ in 0..8 {
                    c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
                }
                TABLE[i as usize] = c;
            }
        });
        let mut c = 0xFFFF_FFFFu32;
        for &b in data {
            c = TABLE[((c ^ b as u32) & 0xFF) as usize] ^ (c >> 8);
        }
        c ^ 0xFFFF_FFFF
    }
}

impl Default for ZipWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl ZipWriter {
    pub fn new() -> Self {
        Self { buf: Vec::new(), entries: Vec::new() }
    }

    fn push(&mut self, v: &[u8]) {
        self.buf.extend_from_slice(v);
    }

    /// Add one file. `name` uses forward slashes. Directories are implicit.
    pub fn add(&mut self, name: &str, data: &[u8]) {
        let offset = self.buf.len() as u64;
        let crc = crc32(data);
        let size = data.len() as u32;
        let name_b = name.as_bytes();
        // local file header
        self.push(&0x0403_4b50_u32.to_le_bytes());
        self.push(&[20, 0]); // version needed
        self.push(&[0, 0]); // flags
        self.push(&0_u16.to_le_bytes()); // method: stored
        self.push(&0_u16.to_le_bytes()); // mod time
        self.push(&0x21_u16.to_le_bytes()); // mod date (1980-01-01)
        self.push(&crc.to_le_bytes());
        self.push(&size.to_le_bytes());
        self.push(&size.to_le_bytes());
        self.push(&(name_b.len() as u16).to_le_bytes());
        self.push(&0_u16.to_le_bytes()); // extra len
        self.push(name_b);
        self.push(data);
        self.entries.push((name.to_string(), offset, crc, size));
    }

    /// Add a file from disk under `arc_name` (forward slashes).
    pub fn add_file(&mut self, disk: &Path, arc_name: &str) -> std::io::Result<()> {
        let data = std::fs::read(disk)?;
        self.add(arc_name, &data);
        Ok(())
    }

    /// Recursively add a directory's files under `arc_prefix`.
    pub fn add_dir(&mut self, dir: &Path, arc_prefix: &str, skip: &dyn Fn(&Path) -> bool) -> std::io::Result<usize> {
        let mut n = 0;
        let mut stack = vec![dir.to_path_buf()];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d)? {
                let p = e?.path();
                if skip(&p) {
                    continue;
                }
                if p.is_dir() {
                    stack.push(p);
                } else {
                    let rel = p.strip_prefix(dir).unwrap_or(&p);
                    let arc = format!("{arc_prefix}/{}", rel.to_string_lossy().replace('\\', "/"));
                    self.add_file(&p, &arc)?;
                    n += 1;
                }
            }
        }
        Ok(n)
    }

    pub fn finish(mut self) -> Vec<u8> {
        let cd_start = self.buf.len() as u64;
        let mut cd = Vec::new();
        for (name, offset, crc, size) in &self.entries {
            let name_b = name.as_bytes();
            cd.extend_from_slice(&0x0201_4b50_u32.to_le_bytes());
            cd.extend_from_slice(&[20, 0]); // version made by
            cd.extend_from_slice(&[20, 0]); // version needed
            cd.extend_from_slice(&[0, 0]); // flags
            cd.extend_from_slice(&0_u16.to_le_bytes()); // stored
            cd.extend_from_slice(&0_u16.to_le_bytes()); // time
            cd.extend_from_slice(&0x21_u16.to_le_bytes()); // date
            cd.extend_from_slice(&crc.to_le_bytes());
            cd.extend_from_slice(&size.to_le_bytes());
            cd.extend_from_slice(&size.to_le_bytes());
            cd.extend_from_slice(&(name_b.len() as u16).to_le_bytes());
            cd.extend_from_slice(&0_u16.to_le_bytes()); // extra
            cd.extend_from_slice(&0_u16.to_le_bytes()); // comment
            cd.extend_from_slice(&0_u16.to_le_bytes()); // disk start
            cd.extend_from_slice(&0_u16.to_le_bytes()); // internal attrs
            cd.extend_from_slice(&0_u32.to_le_bytes()); // external attrs
            cd.extend_from_slice(&(*offset as u32).to_le_bytes()); // local header offset (u32!)
            cd.extend_from_slice(name_b);
        }
        let cd_len = cd.len() as u32;
        self.push(&cd);
        // end of central directory
        self.push(&0x0605_4b50_u32.to_le_bytes());
        self.push(&0_u16.to_le_bytes()); // disk
        self.push(&0_u16.to_le_bytes()); // cd disk
        self.push(&(self.entries.len() as u16).to_le_bytes());
        self.push(&(self.entries.len() as u16).to_le_bytes());
        self.push(&cd_len.to_le_bytes());
        self.push(&(cd_start as u32).to_le_bytes());
        self.push(&0_u16.to_le_bytes()); // comment len
        self.buf
    }
}

/// Default skip predicate for packaging: build dirs, dotfiles, junk.
pub fn skip_build(p: &Path) -> bool {
    let name = p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    if name == "target" || name == "dist" || name.starts_with('.') || name == "Cargo.lock~" {
        return true;
    }
    if let Some(ext) = p.extension() {
        if ext == "png" && p.parent().and_then(|d| d.file_name()).map(|s| s == "test").unwrap_or(false) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_known_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn zip_roundtrip_shape() {
        let mut z = ZipWriter::new();
        z.add("a/b.txt", b"hello world");
        let bytes = z.finish();
        assert_eq!(&bytes[0..4], &[0x50, 0x4b, 0x03, 0x04]);
        assert!(bytes.windows(4).any(|w| w == [0x50, 0x4b, 0x01, 0x02]));
        assert!(bytes.windows(4).any(|w| w == [0x50, 0x4b, 0x05, 0x06]));
    }

    #[test]
    fn skip_predicate() {
        let root = PathBuf::from("/x");
        assert!(skip_build(&root.join("target")));
        assert!(skip_build(&root.join(".git")));
        assert!(!skip_build(&root.join("src")));
    }
}
