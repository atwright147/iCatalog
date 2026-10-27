//! Display metadata from source headers and embedded ICC profiles, without pixel decoding.
use std::io::Cursor;

use image::{ExtendedColorType, ImageDecoder, ImageReader, Limits};

use crate::Result;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ColorInfo {
    pub model: Option<String>,
    pub profile: Option<String>,
}

/// Read source colour information, never infer it from an RGB thumbnail.
pub fn read_color_info(bytes: &[u8]) -> Result<ColorInfo> {
    let mut reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(50_000);
    limits.max_image_height = Some(50_000);
    limits.max_alloc = Some(256 * 1024 * 1024);
    reader.limits(limits);
    let mut decoder = reader.into_decoder()?;
    let model = match decoder.original_color_type() {
        ExtendedColorType::L1
        | ExtendedColorType::L2
        | ExtendedColorType::L4
        | ExtendedColorType::L8
        | ExtendedColorType::L16
        | ExtendedColorType::La1
        | ExtendedColorType::La2
        | ExtendedColorType::La4
        | ExtendedColorType::La8
        | ExtendedColorType::La16 => Some("Gray"),
        ExtendedColorType::Rgb8
        | ExtendedColorType::Rgb16
        | ExtendedColorType::Rgb32F
        | ExtendedColorType::Rgba8
        | ExtendedColorType::Rgba16
        | ExtendedColorType::Rgba32F
        | ExtendedColorType::Rgb1
        | ExtendedColorType::Rgb2
        | ExtendedColorType::Rgb4
        | ExtendedColorType::Rgba1
        | ExtendedColorType::Rgba2
        | ExtendedColorType::Rgba4 => Some("RGB"),
        ExtendedColorType::Cmyk8 | ExtendedColorType::Cmyk16 => Some("CMYK"),
        _ => None,
    };
    let mut info = ColorInfo {
        model: model.map(str::to_owned),
        profile: None,
    };
    // JPEG decoders expose converted RGB for four-component CMYK/YCCK files.
    if jpeg_components(bytes) == Some(4) {
        info.model = Some("CMYK".into());
    }
    if let Ok(Some(profile)) = decoder.icc_profile() {
        if profile.len() <= 16 * 1024 * 1024 {
            if let Some((model, description)) = icc_info(&profile) {
                info.model = model.or(info.model);
                info.profile = Some(description.unwrap_or_else(|| "Embedded ICC profile".into()));
            }
        }
    }
    Ok(info)
}

fn jpeg_components(bytes: &[u8]) -> Option<u8> {
    if bytes.get(..2)? != [0xff, 0xd8] {
        return None;
    }
    let mut pos = 2usize;
    while pos < bytes.len() {
        if *bytes.get(pos)? != 0xff {
            return None;
        }
        while bytes.get(pos) == Some(&0xff) {
            pos = pos.checked_add(1)?;
        }
        let marker = *bytes.get(pos)?;
        pos = pos.checked_add(1)?;
        if matches!(marker, 0xd9 | 0xda) {
            return None;
        }
        if matches!(marker, 0x01 | 0xd0..=0xd8) {
            continue;
        }
        let length = usize::from(u16::from_be_bytes(
            bytes.get(pos..pos.checked_add(2)?)?.try_into().ok()?,
        ));
        if length < 2 {
            return None;
        }
        let end = pos.checked_add(length)?;
        let segment = bytes.get(pos..end)?;
        if matches!(marker, 0xc0..=0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf) {
            return segment.get(7).copied();
        }
        pos = end;
    }
    None
}

fn uint(bytes: &[u8], offset: usize) -> Option<usize> {
    usize::try_from(u32::from_be_bytes(
        bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?,
    ))
    .ok()
}

fn text(value: String) -> Option<String> {
    let value: String = value
        .chars()
        .filter(|c| !c.is_control())
        .take(1024)
        .collect();
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

// ICC header and profileDescriptionTag ('desc'), v2 textDescriptionType or
// v4 multiLocalizedUnicodeType. https://www.color.org/ICC1V42.pdf
fn icc_info(bytes: &[u8]) -> Option<(Option<String>, Option<String>)> {
    let size = uint(bytes, 0)?;
    let bytes = bytes.get(..size)?;
    if bytes.get(36..40)? != b"acsp" {
        return None;
    }
    let model = match bytes.get(16..20)? {
        b"RGB " => Some("RGB"),
        b"CMYK" => Some("CMYK"),
        b"Lab " => Some("Lab"),
        b"GRAY" => Some("Gray"),
        b"XYZ " => Some("XYZ"),
        _ => None,
    }
    .map(str::to_owned);
    let count = uint(bytes, 128)?;
    if count > 4096 {
        return Some((model, None));
    }
    let mut description = None;
    for i in 0..count {
        let entry = 132usize.checked_add(i.checked_mul(12)?)?;
        if bytes.get(entry..entry.checked_add(4)?)? != b"desc" {
            continue;
        }
        let offset = uint(bytes, entry.checked_add(4)?)?;
        let len = uint(bytes, entry.checked_add(8)?)?;
        let data = bytes.get(offset..offset.checked_add(len)?)?;
        description = profile_description(data);
        break;
    }
    Some((model, description))
}

fn profile_description(data: &[u8]) -> Option<String> {
    match data.get(..4)? {
        b"desc" => {
            let length = uint(data, 8)?;
            if length > 4096 {
                return None;
            }
            text(String::from_utf8_lossy(data.get(12..12usize.checked_add(length)?)?).into_owned())
        }
        b"mluc" => {
            let count = uint(data, 8)?;
            let record_size = uint(data, 12)?;
            if count > 256 || record_size < 12 {
                return None;
            }
            let mut fallback = None;
            for i in 0..count {
                let pos = 16usize.checked_add(i.checked_mul(record_size)?)?;
                let len = uint(data, pos.checked_add(4)?)?;
                let offset = uint(data, pos.checked_add(8)?)?;
                if len > 4096 || len % 2 != 0 {
                    continue;
                }
                let bytes = data.get(offset..offset.checked_add(len)?)?;
                let utf16: Vec<u16> = bytes
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|p| u16::from_be_bytes(*p))
                    .collect();
                let value = text(String::from_utf16_lossy(&utf16));
                if data.get(pos..pos.checked_add(2)?) == Some(b"en") && value.is_some() {
                    return value;
                }
                fallback = fallback.or(value);
            }
            fallback
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use image::ImageEncoder;

    #[test]
    fn reads_source_colour_and_embedded_profile() {
        let mut bytes = Vec::new();
        image::codecs::png::PngEncoder::new(&mut bytes)
            .write_image(&[42; 6], 2, 1, ExtendedColorType::Rgb8)
            .unwrap();
        assert_eq!(
            read_color_info(&bytes).unwrap(),
            ColorInfo {
                model: Some("RGB".into()),
                profile: None
            }
        );
        let mut profile = vec![0; 144];
        profile[16..20].copy_from_slice(b"CMYK");
        profile[36..40].copy_from_slice(b"acsp");
        profile[128..132].copy_from_slice(&1u32.to_be_bytes());
        profile[132..136].copy_from_slice(b"desc");
        profile[136..140].copy_from_slice(&144u32.to_be_bytes());
        let name = b"Synthetic print profile\0";
        profile[140..144].copy_from_slice(&(12u32 + name.len() as u32).to_be_bytes());
        profile.extend_from_slice(b"desc\0\0\0\0");
        profile.extend_from_slice(&(name.len() as u32).to_be_bytes());
        profile.extend_from_slice(name);
        let len = profile.len() as u32;
        profile[..4].copy_from_slice(&len.to_be_bytes());
        let mut jpeg = Vec::new();
        let mut encoder = image::codecs::jpeg::JpegEncoder::new(&mut jpeg);
        encoder.set_icc_profile(profile.clone()).unwrap();
        encoder
            .encode(&[42; 6], 2, 1, ExtendedColorType::Rgb8)
            .unwrap();
        assert_eq!(
            read_color_info(&jpeg).unwrap(),
            ColorInfo {
                model: Some("CMYK".into()),
                profile: Some("Synthetic print profile".into())
            }
        );
        for end in 0..profile.len() {
            let _ = icc_info(&profile[..end]);
        }
        profile[128..132].copy_from_slice(&u32::MAX.to_be_bytes());
        assert_eq!(icc_info(&profile), Some((Some("CMYK".into()), None)));
        assert!(read_color_info(b"bad image").is_err());
        assert_eq!(
            jpeg_components(&[0xff, 0xd8, 0xff, 0xc0, 0, 8, 8, 0, 1, 0, 1, 4]),
            Some(4)
        );
    }

    #[test]
    fn unicode_descriptions_and_hostile_offsets() {
        let mut data = b"mluc\0\0\0\0".to_vec();
        data.extend_from_slice(&1u32.to_be_bytes());
        data.extend_from_slice(&12u32.to_be_bytes());
        data.extend_from_slice(b"enGB");
        data.extend_from_slice(&8u32.to_be_bytes());
        data.extend_from_slice(&28u32.to_be_bytes());
        for c in "Labé".encode_utf16() {
            data.extend_from_slice(&c.to_be_bytes());
        }
        assert_eq!(profile_description(&data).as_deref(), Some("Labé"));
        for end in 0..data.len() {
            let _ = profile_description(&data[..end]);
        }
        data[24..28].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(profile_description(&data).is_none());
    }
}
