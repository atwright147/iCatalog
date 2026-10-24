/// Generated 64x32 checkerboard BMP; fixtures contain no personal media.
pub fn bmp(color: [u8; 3]) -> Vec<u8> {
    let (width, height) = (64u32, 32u32);
    let size = 54 + width * height * 3;
    let mut bytes = vec![0; size as usize];
    bytes[0..2].copy_from_slice(b"BM");
    bytes[2..6].copy_from_slice(&size.to_le_bytes());
    bytes[10..14].copy_from_slice(&54u32.to_le_bytes());
    bytes[14..18].copy_from_slice(&40u32.to_le_bytes());
    bytes[18..22].copy_from_slice(&width.to_le_bytes());
    bytes[22..26].copy_from_slice(&height.to_le_bytes());
    bytes[26..28].copy_from_slice(&1u16.to_le_bytes());
    bytes[28..30].copy_from_slice(&24u16.to_le_bytes());
    for y in 0..height {
        for x in 0..width {
            let offset = (54 + (y * width + x) * 3) as usize;
            let pixel = color.map(|c| if (x / 8 + y / 8) % 2 == 0 { c } else { c / 2 });
            bytes[offset..offset + 3].copy_from_slice(&pixel);
        }
    }
    bytes
}
