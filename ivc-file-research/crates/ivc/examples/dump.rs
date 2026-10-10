fn main() {
    for p in std::env::args().skip(1) {
        let d = std::fs::read(&p).unwrap_or_default();
        match ivc::parse(&d) {
            Ok(c) => {
                println!(
                    "{p}: items={} trailer={:x}",
                    c.items.len(),
                    c.trailer.chunk_start
                );
                for i in &c.items {
                    println!(
                        "  {:#x}+{:#x} {} | {} | {:?} {} {}x{} jpeg={} tail={:?}",
                        i.offset,
                        i.len,
                        i.path,
                        i.name,
                        String::from_utf8_lossy(&i.format),
                        i.file_size,
                        i.width,
                        i.height,
                        i.jpeg.is_some(),
                        i.tail_blocks
                            .iter()
                            .map(|b| String::from_utf8_lossy(&b.kind).into_owned())
                            .collect::<Vec<_>>()
                    );
                }
                for ch in &c.chunks {
                    println!(
                        "  chunk {:?} @{:#x} len {}",
                        String::from_utf8_lossy(&ch.kind),
                        ch.offset,
                        ch.payload.len()
                    );
                }
            }
            Err(e) => println!("{p}: ERR {e}"),
        }
    }
}
