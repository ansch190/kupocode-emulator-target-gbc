//! Minimaler PNG-Leser für die Vergleichsbilder der Test-ROMs (ohne externe Crates).
//!
//! Unterstützt Bittiefe 8 (Farbtypen 2, 4, 6), Graustufen und Paletten (Typen 0 und 3,
//! Bittiefe 1/2/4/8),
//! ohne Interlacing. Liefert RGB je Pixel.

/// Bild als RGB-Tripel, zeilenweise.
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<[u8; 3]>,
}

struct Bits<'a> {
    data: &'a [u8],
    pos: usize,
    bit: u32,
}

impl Bits<'_> {
    fn get(&mut self, n: u32) -> Result<u32, String> {
        let mut v = 0;
        for i in 0..n {
            let byte = *self.data.get(self.pos).ok_or("Daten zu kurz")?;
            v |= u32::from((byte >> self.bit) & 1) << i;
            self.bit += 1;
            if self.bit == 8 {
                self.bit = 0;
                self.pos += 1;
            }
        }
        Ok(v)
    }

    fn align(&mut self) {
        if self.bit != 0 {
            self.bit = 0;
            self.pos += 1;
        }
    }
}

/// Kanonischer Huffman-Baum: (Codelänge, Symbol) sortiert.
struct Huffman {
    counts: [u16; 16],
    symbols: Vec<u16>,
}

impl Huffman {
    fn new(lengths: &[u8]) -> Self {
        let mut counts = [0u16; 16];
        for &l in lengths {
            counts[usize::from(l)] += 1;
        }
        counts[0] = 0;
        let mut offs = [0u16; 16];
        for i in 1..16 {
            offs[i] = offs[i - 1] + counts[i - 1];
        }
        let mut symbols = vec![0u16; lengths.len()];
        for (s, &l) in lengths.iter().enumerate() {
            if l != 0 {
                symbols[usize::from(offs[usize::from(l)])] = s as u16;
                offs[usize::from(l)] += 1;
            }
        }
        Self { counts, symbols }
    }

    fn decode(&self, b: &mut Bits) -> Result<u16, String> {
        let (mut code, mut first, mut index) = (0i32, 0i32, 0i32);
        for len in 1..16 {
            code |= b.get(1)? as i32;
            let count = i32::from(self.counts[len]);
            if code - count < first {
                return Ok(self.symbols[(index + code - first) as usize]);
            }
            index += count;
            first += count;
            first <<= 1;
            code <<= 1;
        }
        Err("ungültiger Huffman-Code".into())
    }
}

const LEN_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LEN_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

fn inflate(data: &[u8]) -> Result<Vec<u8>, String> {
    let mut b = Bits {
        data,
        pos: 2, // zlib-Kopf
        bit: 0,
    };
    let mut out = Vec::new();
    loop {
        let last = b.get(1)?;
        match b.get(2)? {
            0 => {
                b.align();
                let len = usize::from(u16::from_le_bytes([data[b.pos], data[b.pos + 1]]));
                b.pos += 4;
                out.extend_from_slice(data.get(b.pos..b.pos + len).ok_or("Block zu kurz")?);
                b.pos += len;
            }
            kind @ (1 | 2) => {
                let (lit, dist) = if kind == 1 {
                    let mut l = [8u8; 288];
                    l[144..256].fill(9);
                    l[256..280].fill(7);
                    (Huffman::new(&l), Huffman::new(&[5u8; 30]))
                } else {
                    let hlit = b.get(5)? as usize + 257;
                    let hdist = b.get(5)? as usize + 1;
                    let hclen = b.get(4)? as usize + 4;
                    const ORDER: [usize; 19] = [
                        16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
                    ];
                    let mut cl = [0u8; 19];
                    for &o in ORDER.iter().take(hclen) {
                        cl[o] = b.get(3)? as u8;
                    }
                    let clh = Huffman::new(&cl);
                    let mut lens = Vec::with_capacity(hlit + hdist);
                    while lens.len() < hlit + hdist {
                        match clh.decode(&mut b)? {
                            s @ 0..=15 => lens.push(s as u8),
                            16 => {
                                let prev = *lens.last().ok_or("Wiederholung ohne Wert")?;
                                for _ in 0..3 + b.get(2)? {
                                    lens.push(prev);
                                }
                            }
                            17 => lens.extend(std::iter::repeat_n(0, 3 + b.get(3)? as usize)),
                            _ => lens.extend(std::iter::repeat_n(0, 11 + b.get(7)? as usize)),
                        }
                    }
                    (Huffman::new(&lens[..hlit]), Huffman::new(&lens[hlit..]))
                };
                loop {
                    let sym = lit.decode(&mut b)?;
                    match sym {
                        0..=255 => out.push(sym as u8),
                        256 => break,
                        _ => {
                            let i = usize::from(sym - 257);
                            let len =
                                usize::from(LEN_BASE[i]) + b.get(u32::from(LEN_EXTRA[i]))? as usize;
                            let d = usize::from(dist.decode(&mut b)?);
                            let dist = usize::from(DIST_BASE[d])
                                + b.get(u32::from(DIST_EXTRA[d]))? as usize;
                            let start = out.len().checked_sub(dist).ok_or("Abstand zu groß")?;
                            for k in 0..len {
                                out.push(out[start + k]);
                            }
                        }
                    }
                }
            }
            _ => return Err("ungültiger Blocktyp".into()),
        }
        if last == 1 {
            return Ok(out);
        }
    }
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = i16::from(a) + i16::from(b) - i16::from(c);
    let (pa, pb, pc) = (
        (p - i16::from(a)).abs(),
        (p - i16::from(b)).abs(),
        (p - i16::from(c)).abs(),
    );
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

pub fn decode(file: &[u8]) -> Result<Image, String> {
    if file.get(..8) != Some(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A][..]) {
        return Err("keine PNG-Datei".into());
    }
    let mut pos = 8;
    let (mut width, mut height, mut depth, mut ctype) = (0u32, 0u32, 0u8, 0u8);
    let mut palette = Vec::new();
    let mut idat = Vec::new();
    while pos + 8 <= file.len() {
        let len = u32::from_be_bytes(file[pos..pos + 4].try_into().unwrap()) as usize;
        let kind = &file[pos + 4..pos + 8];
        let body = &file[pos + 8..pos + 8 + len];
        match kind {
            b"IHDR" => {
                width = u32::from_be_bytes(body[0..4].try_into().unwrap());
                height = u32::from_be_bytes(body[4..8].try_into().unwrap());
                depth = body[8];
                ctype = body[9];
                if body[12] != 0 {
                    return Err("Interlacing nicht unterstützt".into());
                }
            }
            b"PLTE" => palette = body.chunks(3).map(|c| [c[0], c[1], c[2]]).collect(),
            b"IDAT" => idat.extend_from_slice(body),
            b"IEND" => break,
            _ => {}
        }
        pos += 12 + len;
    }
    let raw = inflate(&idat)?;
    let channels = match ctype {
        0 | 3 => 1,
        2 => 3,
        4 => 2,
        6 => 4,
        t => return Err(format!("Farbtyp {t} nicht unterstützt")),
    };
    if !matches!(ctype, 0 | 3) && depth != 8 {
        return Err(format!("Bittiefe {depth} nicht unterstützt"));
    }
    let bpp = (channels * usize::from(depth)).div_ceil(8);
    let stride = (width as usize * channels * usize::from(depth)).div_ceil(8);
    let mut prev = vec![0u8; stride];
    let mut rgb = Vec::with_capacity((width * height) as usize);
    for y in 0..height as usize {
        let row_start = y * (stride + 1);
        let filter = raw[row_start];
        let mut row = raw[row_start + 1..row_start + 1 + stride].to_vec();
        for i in 0..stride {
            let a = if i >= bpp { row[i - bpp] } else { 0 };
            let b = prev[i];
            let c = if i >= bpp { prev[i - bpp] } else { 0 };
            row[i] = row[i].wrapping_add(match filter {
                1 => a,
                2 => b,
                3 => ((u16::from(a) + u16::from(b)) / 2) as u8,
                4 => paeth(a, b, c),
                _ => 0,
            });
        }
        for x in 0..width as usize {
            let px = match ctype {
                0 if depth < 8 => {
                    let bit = x * usize::from(depth);
                    let shift = 8 - usize::from(depth) - bit % 8;
                    let max = (1u16 << depth) - 1;
                    let v = (u16::from(row[bit / 8] >> shift) & max) * 255 / max;
                    [v as u8; 3]
                }
                3 => {
                    let bit = x * usize::from(depth);
                    let byte = row[bit / 8];
                    let shift = 8 - usize::from(depth) - bit % 8;
                    let idx = (byte >> shift) & ((1u16 << depth) - 1) as u8;
                    palette[usize::from(idx)]
                }
                0 | 4 => {
                    let g = row[x * channels];
                    [g, g, g]
                }
                _ => [
                    row[x * channels],
                    row[x * channels + 1],
                    row[x * channels + 2],
                ],
            };
            rgb.push(px);
        }
        prev = row;
    }
    Ok(Image { width, height, rgb })
}
