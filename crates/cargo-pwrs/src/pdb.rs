//! The symbol file a Windows image names. A library or executable built
//! with symbols records the name of its `.pdb` in the CodeView entry of
//! its debug directory, and a debugger searches for that name beside the
//! image. It is read from the bytes, as the class records are, so a
//! library built on another machine is read the same way.

use crate::Error;

/// The file name of the `.pdb` the PE image `bytes` records: `None` for
/// bytes that are not a PE image and for an image that records none, and
/// an error for an image whose headers end early, place its debug
/// directory in no section, or record a path that is not UTF-8 or names
/// no file. Of a recorded path, only the last part.
pub fn recorded(bytes: &[u8]) -> Result<Option<String>, Error> {
    if !bytes.starts_with(b"MZ") {
        return Ok(None);
    }
    let nt = u32_at(bytes, 0x3c)? as usize;
    if span(bytes, nt, 4)? != b"PE\0\0" {
        return Ok(None);
    }
    let sections = u16_at(bytes, nt + 6)? as usize;
    let optional = nt + 24;
    let directories = match u16_at(bytes, optional)? {
        0x10b => optional + 96,
        0x20b => optional + 112,
        other => return Err(Error::msg(format!("its optional header has the magic 0x{other:x}, which PE does not define"))),
    };
    // The debug directory is the seventh data directory, and the number
    // of them comes just before the first.
    if u32_at(bytes, directories - 4)? < 7 {
        return Ok(None);
    }
    let debug = u32_at(bytes, directories + 6 * 8)?;
    let size = u32_at(bytes, directories + 6 * 8 + 4)? as usize;
    if debug == 0 {
        return Ok(None);
    }
    let table = optional + u16_at(bytes, nt + 20)? as usize;
    let mut offset = None;
    for s in 0..sections {
        let header = table + s * 40;
        let virtual_size = u32_at(bytes, header + 8)?;
        let address = u32_at(bytes, header + 12)?;
        let raw_size = u32_at(bytes, header + 16)?;
        let raw = u32_at(bytes, header + 20)?;
        if debug >= address && debug - address < virtual_size.max(raw_size) {
            offset = Some(raw as usize + (debug - address) as usize);
            break;
        }
    }
    let Some(offset) = offset else {
        return Err(Error::msg(format!("its debug directory is at the address 0x{debug:x}, which is in no section")));
    };
    for n in 0..size / 28 {
        let entry = offset + n * 28;
        // Type 2 is CodeView. Its data is "RSDS", a GUID, an age and the
        // path, and the entry gives where that data is in the file.
        if u32_at(bytes, entry + 12)? != 2 {
            continue;
        }
        let data = u32_at(bytes, entry + 24)? as usize;
        if span(bytes, data, 4)? != b"RSDS" {
            continue;
        }
        let rest = match bytes.get(data + 24..) {
            Some(rest) => rest,
            None => return Err(cut()),
        };
        let end = match rest.iter().position(|&b| b == 0) {
            Some(end) => end,
            None => return Err(cut()),
        };
        let path = std::str::from_utf8(&rest[..end]).map_err(|e| Error::msg(format!("the .pdb path it records is not UTF-8: {e}")))?;
        let name = match path.rfind(['\\', '/']) {
            Some(i) => &path[i + 1..],
            None => path,
        };
        if name.is_empty() {
            return Err(Error::msg(format!("the .pdb path it records, {path:?}, names no file")));
        }
        return Ok(Some(name.to_string()));
    }
    Ok(None)
}

fn cut() -> Error {
    Error::msg("it ends inside its PE headers")
}

/// The `len` bytes of `bytes` from `at`.
fn span(bytes: &[u8], at: usize, len: usize) -> Result<&[u8], Error> {
    match at.checked_add(len).and_then(|end| bytes.get(at..end)) {
        Some(part) => Ok(part),
        None => Err(cut()),
    }
}

fn u16_at(bytes: &[u8], at: usize) -> Result<u16, Error> {
    let b = span(bytes, at, 2)?;
    Ok(u16::from_le_bytes([b[0], b[1]]))
}

fn u32_at(bytes: &[u8], at: usize) -> Result<u32, Error> {
    let b = span(bytes, at, 4)?;
    Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

#[cfg(test)]
mod tests {
    use super::recorded;

    /// A PE32+ image with one section, at file offset 0x200 and address
    /// 0x1000, whose debug directory there has one entry of type `kind`
    /// with CodeView data recording `pdb`.
    fn image(kind: u32, pdb: &str) -> Vec<u8> {
        let mut b = vec![0u8; 0x400];
        let mut put = |at: usize, bytes: &[u8]| b[at..at + bytes.len()].copy_from_slice(bytes);
        put(0, b"MZ");
        put(0x3c, &0x80u32.to_le_bytes());
        put(0x80, b"PE\0\0");
        put(0x84, &0x8664u16.to_le_bytes());
        put(0x86, &1u16.to_le_bytes());
        put(0x94, &240u16.to_le_bytes());
        put(0x98, &0x20bu16.to_le_bytes());
        put(0x98 + 108, &16u32.to_le_bytes());
        put(0x98 + 112 + 48, &0x1000u32.to_le_bytes());
        put(0x98 + 112 + 52, &28u32.to_le_bytes());
        let section = 0x98 + 240;
        put(section + 8, &0x200u32.to_le_bytes());
        put(section + 12, &0x1000u32.to_le_bytes());
        put(section + 16, &0x200u32.to_le_bytes());
        put(section + 20, &0x200u32.to_le_bytes());
        put(0x200 + 12, &kind.to_le_bytes());
        put(0x200 + 24, &0x240u32.to_le_bytes());
        put(0x240, b"RSDS");
        put(0x240 + 24, pdb.as_bytes());
        b
    }

    #[test]
    fn the_codeview_entry_names_the_pdb_by_its_last_part() {
        for (path, name) in [
            ("pwrs_example_hello.pdb", "pwrs_example_hello.pdb"),
            (r"C:\build\deps\hello_helper.pdb", "hello_helper.pdb"),
            ("/pwrs/module/net10.0/Pwrs.Runtime.pdb", "Pwrs.Runtime.pdb"),
        ] {
            assert_eq!(recorded(&image(2, path)).expect("a readable image").as_deref(), Some(name), "{path}");
        }
    }

    #[test]
    fn bytes_that_record_no_pdb_give_none() {
        assert_eq!(recorded(&image(16, "x.pdb")).expect("a readable image"), None, "a debug entry of another type");
        let mut empty = image(2, "x.pdb");
        empty[0x98 + 112 + 48..0x98 + 112 + 56].fill(0);
        assert_eq!(recorded(&empty).expect("a readable image"), None, "an image whose debug directory is empty");
        assert_eq!(recorded(b"\x7fELF\x02\x01\x01\0").expect("bytes that are no image"), None, "an ELF library");
    }

    #[test]
    fn an_image_that_cannot_be_read_is_an_error() {
        assert!(recorded(&image(2, "x.pdb")[..0x100]).is_err(), "an image cut short");
        assert!(recorded(&image(2, "")).is_err(), "an empty path");
        let mut not_text = image(2, "x.pdb");
        not_text[0x240 + 24] = 0xff;
        assert!(recorded(&not_text).is_err(), "a path that is not UTF-8");
    }
}
