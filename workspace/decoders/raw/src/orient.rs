/// Minimal TIFF/EXIF orientation reader for camera RAW containers.
///
/// rawler 0.7.2 exposes the sensor pixels but reports `orientation = 1` for
/// several real files (notably 3FR/MEF/Leica RAW). Other viewers apply the
/// TIFF orientation tag, so we read tag 274 ourselves and only use it when
/// rawler did not report a real orientation.
pub(crate) fn from_bytes(data: &[u8]) -> Option<i32> {
    if data.len() < 8 {
        return None;
    }
    if &data[0..4] == b"II*\0" {
        let off = u32::from_le_bytes(data[4..8].try_into().ok()?) as usize;
        return read_ifd(data, off, true).or_else(|| scan_le(data));
    }
    if &data[0..4] == b"MM\0*" {
        let off = u32::from_be_bytes(data[4..8].try_into().ok()?) as usize;
        return read_ifd(data, off, false);
    }
    // Leica's `.RAW` starts with `IIU\0`, not a TIFF magic, but its IFD still
    // carries the standard little-endian tag. Keep the scan bounded.
    scan_le(&data[..data.len().min(4096)])
}

fn read_ifd(data: &[u8], off: usize, le: bool) -> Option<i32> {
    if off + 2 > data.len() {
        return None;
    }
    let count = if le {
        u16::from_le_bytes(data[off..off + 2].try_into().ok()?) as usize
    } else {
        u16::from_be_bytes(data[off..off + 2].try_into().ok()?) as usize
    };
    let entries = off.checked_add(2)?;
    if entries.checked_add(count.checked_mul(12)?)? > data.len() {
        return None;
    }
    for i in 0..count {
        let e = entries + i * 12;
        let tag = if le {
            u16::from_le_bytes(data[e..e + 2].try_into().ok()?)
        } else {
            u16::from_be_bytes(data[e..e + 2].try_into().ok()?)
        };
        if tag != 274 {
            continue;
        }
        let ty = if le {
            u16::from_le_bytes(data[e + 2..e + 4].try_into().ok()?)
        } else {
            u16::from_be_bytes(data[e + 2..e + 4].try_into().ok()?)
        };
        if ty != 3 {
            return None;
        }
        let v = if le {
            u16::from_le_bytes(data[e + 8..e + 10].try_into().ok()?)
        } else {
            u16::from_be_bytes(data[e + 8..e + 10].try_into().ok()?)
        };
        return (1..=8).contains(&v).then_some(v as i32);
    }
    None
}

/// Bounded fallback for `IIU\0`-style vendor headers: find an inline
/// little-endian SHORT orientation entry. This is only reached after the
/// standard TIFF parse fails.
fn scan_le(data: &[u8]) -> Option<i32> {
    let mut i = 0;
    while i + 10 <= data.len() {
        if data[i..i + 4] == [0x12, 0x01, 0x03, 0x00]
            && data[i + 4..i + 8] == [0x01, 0x00, 0x00, 0x00]
        {
            let v = u16::from_le_bytes([data[i + 8], data[i + 9]]);
            if (1..=8).contains(&v) {
                return Some(v as i32);
            }
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::from_bytes;

    #[test]
    fn reads_little_and_big_endian_orientation() {
        let mut le = vec![0u8; 32];
        le[0..4].copy_from_slice(b"II*\0");
        le[4..8].copy_from_slice(&8u32.to_le_bytes());
        le[8..10].copy_from_slice(&1u16.to_le_bytes());
        le[10..12].copy_from_slice(&274u16.to_le_bytes());
        le[12..14].copy_from_slice(&3u16.to_le_bytes());
        le[14..18].copy_from_slice(&1u32.to_le_bytes());
        le[18..20].copy_from_slice(&6u16.to_le_bytes());
        assert_eq!(from_bytes(&le), Some(6));

        let mut be = vec![0u8; 32];
        be[0..4].copy_from_slice(b"MM\0*");
        be[4..8].copy_from_slice(&8u32.to_be_bytes());
        be[8..10].copy_from_slice(&1u16.to_be_bytes());
        be[10..12].copy_from_slice(&274u16.to_be_bytes());
        be[12..14].copy_from_slice(&3u16.to_be_bytes());
        be[14..18].copy_from_slice(&1u32.to_be_bytes());
        be[18..20].copy_from_slice(&8u16.to_be_bytes());
        assert_eq!(from_bytes(&be), Some(8));
    }
}
