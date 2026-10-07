//! Pixel density stored in PNG and JPEG files, in pixels per inch.

use png::{Info, Unit};

const CM_PER_INCH: f64 = 2.54;
const EXIF_RESOLUTION_X: u16 = 282;
const EXIF_RESOLUTION_Y: u16 = 283;
const EXIF_RESOLUTION_UNIT: u16 = 296;

/// Pixels per inch along each axis. Both are positive and finite.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Density {
    pub x: f64,
    pub y: f64,
}

impl Density {
    fn new(x: f64, y: f64) -> Option<Self> {
        let valid = |v: f64| v.is_finite() && v > 0.0;
        (valid(x) && valid(y)).then_some(Self { x, y })
    }
}

/// The `pHYs` density of a PNG, if it is in meters.
pub fn png(info: &Info) -> Option<Density> {
    let dims = info.pixel_dims.filter(|dims| dims.unit == Unit::Meter)?;
    Density::new(f64::from(dims.xppu) * 0.0254, f64::from(dims.yppu) * 0.0254)
}

/// The JFIF density of a JPEG, else its EXIF resolution. Aspect-only JFIF density does not count.
pub fn jpeg(data: &[u8]) -> Option<Density> {
    let mut exif = None;
    let mut rest = data.get(2..)?;
    while let [0xff, marker, length_high, length_low, ..] = *rest {
        let length = usize::from(u16::from_be_bytes([length_high, length_low]));
        let segment = rest.get(4..2 + length)?;
        match marker {
            0xe0 if segment.starts_with(b"JFIF\0") => {
                if let Some(density) = jfif(segment) {
                    return Some(density);
                }
            }
            0xe1 if exif.is_none() => exif = segment.strip_prefix(b"Exif\0\0").and_then(self::exif),
            0xda | 0xd9 => break,
            _ => {}
        }
        rest = &rest[2 + length..];
    }
    exif
}

/// Reads the units and densities after the `JFIF\0` identifier and version.
fn jfif(segment: &[u8]) -> Option<Density> {
    let [.., units, xh, xl, yh, yl] = segment.get(..12)? else {
        return None;
    };
    let scale = match units {
        1 => 1.0,
        2 => CM_PER_INCH,
        _ => return None,
    };
    Density::new(
        f64::from(u16::from_be_bytes([*xh, *xl])) * scale,
        f64::from(u16::from_be_bytes([*yh, *yl])) * scale,
    )
}

/// Reads the resolution tags of the first image directory in a TIFF structure.
fn exif(tiff: &[u8]) -> Option<Density> {
    let big = match tiff.get(..2)? {
        b"MM" => true,
        b"II" => false,
        _ => return None,
    };
    let u16_at = |at: usize| {
        let bytes: [u8; 2] = tiff.get(at..at + 2)?.try_into().ok()?;
        Some(if big {
            u16::from_be_bytes(bytes)
        } else {
            u16::from_le_bytes(bytes)
        })
    };
    let u32_at = |at: usize| {
        let bytes: [u8; 4] = tiff.get(at..at + 4)?.try_into().ok()?;
        Some(if big {
            u32::from_be_bytes(bytes)
        } else {
            u32::from_le_bytes(bytes)
        })
    };
    let directory = usize::try_from(u32_at(4)?).ok()?;
    let (mut x, mut y, mut unit) = (None, None, 2);
    for index in 0..usize::from(u16_at(directory)?) {
        let entry = directory + 2 + index * 12;
        let rational = || {
            let at = usize::try_from(u32_at(entry + 8)?).ok()?;
            Some(f64::from(u32_at(at)?) / f64::from(u32_at(at + 4)?))
        };
        match u16_at(entry)? {
            EXIF_RESOLUTION_X => x = rational(),
            EXIF_RESOLUTION_Y => y = rational(),
            EXIF_RESOLUTION_UNIT => unit = u16_at(entry + 8)?,
            _ => {}
        }
    }
    let scale = match unit {
        2 => 1.0,
        3 => CM_PER_INCH,
        _ => return None,
    };
    Density::new(x? * scale, y? * scale)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jfif_segment(units: u8, x: u16, y: u16) -> Vec<u8> {
        let mut data = vec![0xff, 0xd8, 0xff, 0xe0, 0, 16];
        data.extend_from_slice(b"JFIF\0\x01\x01");
        data.push(units);
        data.extend([x.to_be_bytes(), y.to_be_bytes()].concat());
        data.extend([0, 0]);
        data
    }

    #[test]
    fn reads_jfif_density_in_inches_or_centimeters_and_skips_aspect_only() {
        assert_eq!(jpeg(&jfif_segment(1, 300, 300)), Density::new(300.0, 300.0));
        assert_eq!(jpeg(&jfif_segment(2, 100, 100)), Density::new(254.0, 254.0));
        assert_eq!(jpeg(&jfif_segment(0, 1, 1)), None);
    }

    #[test]
    fn falls_back_to_exif_resolution() {
        let mut tiff = b"MM\0*\0\0\0\x08\0\x03".to_vec();
        let rational_at = 8 + 2 + 3 * 12 + 4;
        for (tag, offset) in [(282u16, rational_at), (283, rational_at + 8)] {
            tiff.extend(tag.to_be_bytes());
            tiff.extend([0, 5, 0, 0, 0, 1]);
            tiff.extend((offset as u32).to_be_bytes());
        }
        tiff.extend([0x01, 0x28, 0, 3, 0, 0, 0, 1, 0, 3, 0, 0]);
        tiff.extend([0, 0, 0, 0]);
        for dots_per_unit in [200u32, 100] {
            tiff.extend(dots_per_unit.to_be_bytes());
            tiff.extend(1u32.to_be_bytes());
        }
        let mut data = jfif_segment(0, 1, 1);
        data.extend([0xff, 0xe1]);
        data.extend((tiff.len() as u16 + 8).to_be_bytes());
        data.extend(b"Exif\0\0");
        data.extend(tiff);
        assert_eq!(jpeg(&data), Density::new(508.0, 254.0));
    }
}
