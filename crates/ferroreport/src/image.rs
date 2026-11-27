//! Image loading and decoding.
//!
//! Images come from template resources (paths relative to the resource directories) or from
//! bytes produced by an expression (base64 data from the data source). Network locations are
//! never fetched, and paths cannot escape the resource directories, so a template from an
//! untrusted source cannot read arbitrary files.

use crate::error::{Error, Result};
use base64::Engine;
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

/// Pixel data ready to be embedded in a PDF.
#[derive(Debug)]
pub enum Pixels {
    /// A JPEG file, embedded as is.
    Jpeg { data: Vec<u8>, components: u8 },
    /// Decoded 8-bit samples (gray or RGB) with an optional 8-bit alpha channel.
    Raw { samples: Vec<u8>, gray: bool, alpha: Option<Vec<u8>> },
    /// An SVG drawing (its size in points, one SVG pixel to a point).
    Svg(Arc<crate::svg::SvgImage>),
}

/// Image ids from here up belong to the bitmaps of charts drawn as SVG
/// (`chart::bitmap`): the PDF masks their transparent pixels, which are black, with a color
/// key, as JasperReports' PDFs do, rather than with a soft mask.
pub(crate) const KEYED_IDS: usize = usize::MAX / 2;

#[derive(Debug)]
pub struct ImageData {
    /// Stable identifier within a render (used to share one PDF object between uses).
    pub id: usize,
    pub width: u32,
    pub height: u32,
    pub pixels: Pixels,
}

/// Loads and caches the resources of one render: decoded images and parsed subreports.
#[derive(Debug, Default)]
pub struct ImageStore {
    roots: Vec<PathBuf>,
    /// Resources given in memory, by relative path (looked up before the directories).
    files: HashMap<String, Arc<[u8]>>,
    by_key: HashMap<String, Arc<ImageData>>,
    next_id: usize,
    /// Parsed subreport templates by location.
    reports: HashMap<String, Arc<crate::model::Report>>,
    /// Subreports being filled right now (guards against runaway recursion).
    pub(crate) depth: usize,
    /// Subreports, tables and lists filled in the current render (guards against templates
    /// that multiply their subreports at every level).
    pub(crate) runs: usize,
    /// Texts are filled without the data only spreadsheet and text exporters use.
    pub(crate) skip_cells: bool,
    /// When the current render must be done, and the time it was given.
    pub(crate) deadline: Option<(std::time::Instant, std::time::Duration)>,
    /// Most rows the current render may read, all its subreports and datasets together.
    pub(crate) max_rows: Option<usize>,
    /// Rows the current render has read so far.
    pub(crate) rows_read: usize,
    /// The database subreports and datasets with a connection get their rows from.
    pub(crate) connection: Option<crate::sql::Shared>,
}

impl ImageStore {
    pub fn new() -> ImageStore {
        ImageStore::default()
    }

    /// Adds a directory relative image paths are resolved against.
    pub fn add_root(&mut self, dir: impl Into<PathBuf>) {
        self.roots.push(dir.into());
    }

    /// Adds a resource held in memory under a relative path (`"logo.png"`, `"sub/lines.jrxml"`).
    pub fn add_file(&mut self, name: &str, data: impl Into<Arc<[u8]>>) {
        self.files.insert(normalize_name(name), data.into());
    }

    /// The texts of a report's resource bundle: `<base>.properties` and the more specific
    /// `<base>_<language>.properties` and `<base>_<language>_<COUNTRY>.properties` for the
    /// locale, the more specific winning. None when no such file is found.
    pub fn load_bundle(&self, base: &str, locale_tag: &str) -> Option<std::collections::HashMap<String, String>> {
        // A bundle is named like a Java class (`com.acme.texts`) or like a file (`texts`).
        let (language, country) = locale_tag.split_once('_').unwrap_or((locale_tag, ""));
        let mut names = vec![String::new(), format!("_{language}")];
        if !country.is_empty() {
            names.push(format!("_{language}_{country}"));
        }
        let mut texts = None;
        for base in [base.to_string(), base.replace('.', "/")] {
            for suffix in &names {
                if let Ok(bytes) = self.read(&format!("{base}{suffix}.properties")) {
                    texts
                        .get_or_insert_with(std::collections::HashMap::new)
                        .extend(crate::format::message::parse_properties(&bytes));
                }
            }
            if texts.is_some() {
                break;
            }
        }
        texts
    }

    /// A resource by relative location, when there is one.
    pub(crate) fn read_resource(&self, location: &str) -> Option<Vec<u8>> {
        self.read(location).ok()
    }

    /// Every copy of a resource: the one in memory and the one in each directory that has it,
    /// as a Java class path gives every `jasperreports_extension.properties` on it.
    pub(crate) fn read_every(&self, location: &str) -> Vec<Vec<u8>> {
        if check_relative(location).is_err() {
            return Vec::new();
        }
        let mut out: Vec<Vec<u8>> = self.files.get(&normalize_name(location)).map(|d| d.to_vec()).into_iter().collect();
        for root in &self.roots {
            let path = root.join(location);
            let inside = match (path.canonicalize(), root.canonicalize()) {
                (Ok(real), Ok(real_root)) => path.is_file() && real.starts_with(&real_root),
                _ => false,
            };
            if let Some(bytes) = inside.then(|| std::fs::read(&path).ok()).flatten() {
                out.push(bytes);
            }
        }
        out
    }

    /// Reads a resource by relative location: in-memory files first, then the directories.
    fn read(&self, location: &str) -> Result<Vec<u8>> {
        check_relative(location)?;
        if let Some(data) = self.files.get(&normalize_name(location)) {
            return Ok(data.to_vec());
        }
        let path = self.resolve(location)?;
        std::fs::read(&path).map_err(|e| Error::Data(format!("`{location}`: {e}")))
    }

    /// Resolves a location string (a relative path, or a `data:` URI) to an image.
    pub fn load_location(&mut self, location: &str) -> Result<Arc<ImageData>> {
        if let Some(img) = self.by_key.get(location) {
            return Ok(img.clone());
        }
        let bytes = if let Some(data) = location.strip_prefix("data:") {
            let (_, b64) =
                data.split_once(";base64,").ok_or_else(|| Error::Data("only base64 data URIs are supported".into()))?;
            decode_base64(b64)?
        } else if location.contains("://") {
            return Err(Error::Unsupported(format!("image `{location}`: remote images are not fetched")));
        } else {
            self.read(location)?
        };
        let img = self.decode(&bytes)?;
        self.by_key.insert(location.to_string(), img.clone());
        Ok(img)
    }

    /// Loads a subreport template. A compiled `.jasper` location is resolved to the `.jrxml`
    /// source next to it (or a `.ferro` one): compiled templates are never deserialized.
    pub fn load_report(&mut self, location: &str) -> Result<Arc<crate::model::Report>> {
        if let Some(r) = self.reports.get(location) {
            return Ok(r.clone());
        }
        if location.contains("://") {
            return Err(Error::Unsupported(format!("subreport `{location}`: remote templates are not fetched")));
        }
        let (source, bytes) = match location.strip_suffix(".jasper") {
            Some(stem) => {
                let jrxml = format!("{stem}.jrxml");
                match self.read(&jrxml) {
                    Ok(b) => (jrxml, b),
                    Err(e) => {
                        let ferro = format!("{stem}.ferro");
                        match self.read(&ferro) {
                            Ok(b) => (ferro, b),
                            Err(_) => return Err(e),
                        }
                    }
                }
            }
            None => (location.to_string(), self.read(location)?),
        };
        let text = String::from_utf8(bytes).map_err(|_| Error::Data(format!("subreport `{source}` is not UTF-8")))?;
        let report = Arc::new(crate::jrxml::parse_report_with(&text, &|l| self.read(l).ok())?);
        self.reports.insert(location.to_string(), report.clone());
        Ok(report)
    }

    /// Decodes image bytes (PNG or JPEG).
    pub fn load_bytes(&mut self, bytes: &[u8]) -> Result<Arc<ImageData>> {
        let key = format!("bytes:{:016x}:{}", fnv(bytes), bytes.len());
        if let Some(img) = self.by_key.get(&key) {
            return Ok(img.clone());
        }
        let img = self.decode(bytes)?;
        self.by_key.insert(key, img.clone());
        Ok(img)
    }

    fn resolve(&self, location: &str) -> Result<PathBuf> {
        let rel = Path::new(location);
        // Without resource directories nothing is read from disk (not even the working
        // directory): only in-memory resources are available.
        // The file must really be inside the directory: a symbolic link pointing elsewhere
        // is refused.
        for root in &self.roots {
            let path = root.join(rel);
            if !path.is_file() {
                continue;
            }
            let (Ok(real), Ok(real_root)) = (path.canonicalize(), root.canonicalize()) else { continue };
            if !real.starts_with(&real_root) {
                return Err(Error::Unsupported(format!("`{location}` links outside the resource directory and is not read")));
            }
            return Ok(path);
        }
        Err(Error::Data(format!("`{location}` not found in the resource directories")))
    }

    fn decode(&mut self, bytes: &[u8]) -> Result<Arc<ImageData>> {
        let id = self.next_id;
        self.next_id += 1;
        let img = if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
            decode_png(bytes, id)?
        } else if bytes.starts_with(&[0xFF, 0xD8]) {
            decode_jpeg(bytes, id)?
        } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
            decode_gif(bytes, id)?
        } else if crate::svg::sniff(bytes) {
            decode_svg(bytes, id)?
        } else {
            return Err(Error::Unsupported("image format (only PNG, JPEG, GIF and SVG are supported)".into()));
        };
        Ok(Arc::new(img))
    }
}

fn decode_svg(bytes: &[u8], id: usize) -> Result<ImageData> {
    let svg = crate::svg::parse(bytes)?;
    let size = svg.tree.size();
    // JasperReports sizes an SVG in whole points, dropping the fraction.
    Ok(ImageData {
        id,
        width: (size.width() as u32).max(1),
        height: (size.height() as u32).max(1),
        pixels: Pixels::Svg(Arc::new(svg)),
    })
}

/// A picture inside an SVG (PNG, JPEG or GIF), with an identifier no store hands out.
pub(crate) fn decode_embedded(bytes: &[u8]) -> Result<Arc<ImageData>> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    // Half the range up: far from the ones a store counts from 0, on 32-bit targets too.
    static NEXT: AtomicUsize = AtomicUsize::new(usize::MAX / 2 + 1);
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let img = if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        decode_png(bytes, id)?
    } else if bytes.starts_with(&[0xFF, 0xD8]) {
        decode_jpeg(bytes, id)?
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        decode_gif(bytes, id)?
    } else {
        return Err(Error::Unsupported("picture inside an SVG".into()));
    };
    Ok(Arc::new(img))
}

pub fn decode_base64(s: &str) -> Result<Vec<u8>> {
    let clean: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    base64::engine::general_purpose::STANDARD
        .decode(clean.as_bytes())
        .map_err(|e| Error::Data(format!("invalid base64 data: {e}")))
}

fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ *b as u64).wrapping_mul(0x0100_0000_01b3))
}

/// Largest image decoded, in pixels (about 16000 × 4000; 400 MB at most while decoding).
const MAX_PIXELS: u64 = 64_000_000;

fn decode_png(bytes: &[u8], id: usize) -> Result<ImageData> {
    let mut decoder = png::Decoder::new(bytes);
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|e| Error::Data(format!("PNG: {e}")))?;
    // The header says how big the image is before anything is decoded: refuse sizes that
    // would take more memory than any report image needs (a small file can claim billions
    // of pixels).
    let (w, h) = (reader.info().width as u64, reader.info().height as u64);
    if w * h > MAX_PIXELS || reader.output_buffer_size() as u64 > MAX_PIXELS * 8 {
        return Err(Error::Data(format!("PNG: {w}×{h} pixels is larger than the {MAX_PIXELS} pixels allowed")));
    }
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).map_err(|e| Error::Data(format!("PNG: {e}")))?;
    let buf = &buf[..info.buffer_size()];
    let (gray, has_alpha, channels) = match info.color_type {
        png::ColorType::Grayscale => (true, false, 1),
        png::ColorType::GrayscaleAlpha => (true, true, 2),
        png::ColorType::Rgb => (false, false, 3),
        png::ColorType::Rgba => (false, true, 4),
        png::ColorType::Indexed => return Err(Error::Data("PNG: unexpanded palette".into())),
    };
    let color_channels = if gray { 1 } else { 3 };
    let pixels = (info.width * info.height) as usize;
    let mut samples = Vec::with_capacity(pixels * color_channels);
    let mut alpha = if has_alpha { Some(Vec::with_capacity(pixels)) } else { None };
    for px in buf.chunks_exact(channels) {
        samples.extend_from_slice(&px[..color_channels]);
        if let Some(a) = &mut alpha {
            a.push(px[channels - 1]);
        }
    }
    // Java's ImageIO decodes gray PNGs into a linear gray color space and JasperReports
    // embeds them converted to sRGB, which brightens mid tones; do the same.
    let (samples, gray) = if gray {
        let lut: Vec<u8> = (0..=255u32)
            .map(|v| {
                let l = v as f64 / 255.0;
                let s = if l <= 0.003_130_8 { 12.92 * l } else { 1.055 * l.powf(1.0 / 2.4) - 0.055 };
                (s * 255.0).round().clamp(0.0, 255.0) as u8
            })
            .collect();
        (
            samples
                .iter()
                .flat_map(|&v| {
                    let c = lut[v as usize];
                    [c, c, c]
                })
                .collect(),
            false,
        )
    } else {
        (samples, gray)
    };
    // A fully opaque alpha channel is dropped.
    if alpha.as_ref().is_some_and(|a| a.iter().all(|&v| v == 255)) {
        alpha = None;
    }
    Ok(ImageData { id, width: info.width, height: info.height, pixels: Pixels::Raw { samples, gray, alpha } })
}

/// The first picture of a GIF file, as JasperReports takes it (its other frames, for
/// animations, are not drawn). The transparent color, if there is one, becomes transparent.
fn decode_gif(bytes: &[u8], id: usize) -> Result<ImageData> {
    let bad = |what: &str| Error::Data(format!("GIF: {what}"));
    let byte = |i: usize| bytes.get(i).copied().ok_or_else(|| bad("the file ends too soon"));
    let word = |i: usize| -> Result<usize> { Ok(byte(i)? as usize | (byte(i + 1)? as usize) << 8) };
    let table = |at: usize, packed: u8| -> Result<Vec<[u8; 3]>> {
        let n = 2usize << (packed & 7);
        let raw = bytes.get(at..at + 3 * n).ok_or_else(|| bad("the color table is cut short"))?;
        Ok(raw.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect())
    };
    let screen_packed = byte(10)?;
    let mut at = 13;
    let global = if screen_packed & 0x80 != 0 {
        let t = table(at, screen_packed)?;
        at += 3 * t.len();
        Some(t)
    } else {
        None
    };
    let mut transparent = None;
    loop {
        match byte(at)? {
            // An extension: a graphic control block may name the transparent color.
            0x21 => {
                if byte(at + 1)? == 0xF9 && byte(at + 2)? >= 4 && byte(at + 3)? & 1 != 0 {
                    transparent = Some(byte(at + 6)?);
                }
                at += 2;
                loop {
                    let n = byte(at)? as usize;
                    at += 1 + n;
                    if n == 0 {
                        break;
                    }
                }
            }
            0x2C => break,
            0x3B => return Err(bad("there is no picture in it")),
            other => return Err(bad(&format!("unknown block {other:#04x}"))),
        }
    }
    let (width, height, packed) = (word(at + 5)?, word(at + 7)?, byte(at + 9)?);
    if width == 0 || height == 0 || width as u64 * height as u64 > MAX_PIXELS {
        return Err(bad(&format!("{width}×{height} pixels is not an image size that is allowed")));
    }
    at += 10;
    let local;
    let colors = if packed & 0x80 != 0 {
        local = table(at, packed)?;
        at += 3 * local.len();
        &local
    } else {
        global.as_ref().ok_or_else(|| bad("there is no color table"))?
    };
    let min_size = byte(at)?;
    if !(1..=11).contains(&min_size) {
        return Err(bad("the compressed data is not valid"));
    }
    at += 1;
    let mut data = Vec::new();
    loop {
        let n = byte(at)? as usize;
        if n == 0 {
            break;
        }
        data.extend_from_slice(bytes.get(at + 1..at + 1 + n).ok_or_else(|| bad("the compressed data is cut short"))?);
        at += 1 + n;
    }
    let indexes = lzw(&data, min_size, width * height).ok_or_else(|| bad("the compressed data is not valid"))?;
    // Interlaced rows come in four passes: every 8th from 0, every 8th from 4, every 4th
    // from 2, every 2nd from 1.
    let order: Vec<usize> = if packed & 0x40 != 0 {
        [(0, 8), (4, 8), (2, 4), (1, 2)].iter().flat_map(|&(start, step)| (start..height).step_by(step)).collect()
    } else {
        (0..height).collect()
    };
    let mut samples = vec![0u8; width * height * 3];
    let mut alpha = transparent.map(|_| vec![255u8; width * height]);
    for (source, &row) in order.iter().enumerate() {
        for x in 0..width {
            let index = indexes.get(source * width + x).copied().unwrap_or(0);
            let at = row * width + x;
            let color = colors.get(index as usize).copied().unwrap_or([0, 0, 0]);
            samples[at * 3..at * 3 + 3].copy_from_slice(&color);
            if let (Some(a), Some(t)) = (&mut alpha, transparent) {
                if index == t {
                    a[at] = 0;
                }
            }
        }
    }
    Ok(ImageData { id, width: width as u32, height: height as u32, pixels: Pixels::Raw { samples, gray: false, alpha } })
}

/// Decodes GIF's variable-length LZW into at most `limit` color indexes.
fn lzw(data: &[u8], min_size: u8, limit: usize) -> Option<Vec<u8>> {
    let clear = 1usize << min_size;
    let end = clear + 1;
    // Each code: the code before it (or none) and its last index; its length.
    let mut prefix: Vec<u16> = Vec::with_capacity(4096);
    let mut last: Vec<u8> = Vec::with_capacity(4096);
    let mut length: Vec<u16> = Vec::with_capacity(4096);
    let reset = |prefix: &mut Vec<u16>, last: &mut Vec<u8>, length: &mut Vec<u16>| {
        prefix.clear();
        last.clear();
        length.clear();
        for i in 0..clear + 2 {
            prefix.push(u16::MAX);
            last.push(i as u8);
            length.push(1);
        }
    };
    reset(&mut prefix, &mut last, &mut length);
    let mut out: Vec<u8> = Vec::with_capacity(limit);
    let (mut size, mut bits, mut held, mut pos) = (min_size as u32 + 1, 0u32, 0u32, 0usize);
    let mut previous: Option<usize> = None;
    let first_of = |code: usize, prefix: &[u16], last: &[u8]| {
        let mut c = code;
        while prefix[c] != u16::MAX {
            c = prefix[c] as usize;
        }
        last[c]
    };
    while out.len() < limit {
        while bits < size {
            let Some(&b) = data.get(pos) else { return Some(out) };
            held |= (b as u32) << bits;
            bits += 8;
            pos += 1;
        }
        let code = (held & ((1 << size) - 1)) as usize;
        held >>= size;
        bits -= size;
        if code == clear {
            reset(&mut prefix, &mut last, &mut length);
            size = min_size as u32 + 1;
            previous = None;
            continue;
        }
        if code == end {
            break;
        }
        let known = code < prefix.len();
        if let Some(p) = previous {
            if !known && code != prefix.len() {
                return None;
            }
            if prefix.len() < 4096 {
                let first = if known { first_of(code, &prefix, &last) } else { first_of(p, &prefix, &last) };
                prefix.push(p as u16);
                last.push(first);
                length.push(length[p] + 1);
                if prefix.len() == 1 << size && size < 12 {
                    size += 1;
                }
            }
        } else if !known {
            return None;
        }
        // Write the code's indexes, last first, into their place.
        let n = length[code] as usize;
        let start = out.len();
        out.resize(start + n, 0);
        let mut c = code;
        for i in (0..n).rev() {
            out[start + i] = last[c];
            c = if prefix[c] == u16::MAX { c } else { prefix[c] as usize };
        }
        previous = Some(code);
    }
    out.truncate(limit);
    Some(out)
}

fn decode_jpeg(bytes: &[u8], id: usize) -> Result<ImageData> {
    // Walk the markers up to the first start-of-frame to read the dimensions.
    let mut i = 2;
    while i + 4 <= bytes.len() {
        if bytes[i] != 0xFF {
            i += 1;
            continue;
        }
        let marker = bytes[i + 1];
        if marker == 0xD8 || marker == 0x01 || (0xD0..=0xD7).contains(&marker) {
            i += 2;
            continue;
        }
        let len = u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;
        let is_sof = matches!(marker, 0xC0..=0xCF) && !matches!(marker, 0xC4 | 0xC8 | 0xCC);
        if is_sof && i + 9 < bytes.len() {
            let height = u16::from_be_bytes([bytes[i + 5], bytes[i + 6]]) as u32;
            let width = u16::from_be_bytes([bytes[i + 7], bytes[i + 8]]) as u32;
            let components = bytes[i + 9];
            return Ok(ImageData { id, width, height, pixels: Pixels::Jpeg { data: bytes.to_vec(), components } });
        }
        i += 2 + len;
    }
    Err(Error::Data("JPEG: no frame header found".into()))
}

/// Refuses absolute paths and paths that climb out of the resource directories.
fn check_relative(location: &str) -> Result<()> {
    let rel = Path::new(location);
    let escapes = rel.is_absolute()
        || location.starts_with('/')
        || location.starts_with('\\')
        || rel.components().any(|c| matches!(c, Component::ParentDir | Component::RootDir | Component::Prefix(_)))
        || location.split(['/', '\\']).any(|part| part == "..");
    if escapes {
        return Err(Error::Unsupported(format!("`{location}`: paths must be relative to the template's resource directory")));
    }
    Ok(())
}

fn normalize_name(name: &str) -> String {
    name.replace('\\', "/").trim_start_matches("./").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_paths_outside_the_resource_directory() {
        let mut store = ImageStore::new();
        store.add_root(std::env::temp_dir());
        for bad in ["/etc/passwd", "../secret.png", "a/../../b.png", "http://example.com/x.png"] {
            assert!(store.load_location(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn refuses_images_claiming_huge_sizes() {
        // A valid PNG header claiming 60000×60000 pixels, with almost no data.
        fn chunk(kind: &[u8], data: &[u8]) -> Vec<u8> {
            let mut c = (data.len() as u32).to_be_bytes().to_vec();
            c.extend_from_slice(kind);
            c.extend_from_slice(data);
            let mut crc_input = kind.to_vec();
            crc_input.extend_from_slice(data);
            let crc = crc_input.iter().fold(!0u32, |mut c, &b| {
                c ^= b as u32;
                for _ in 0..8 {
                    c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
                }
                c
            });
            c.extend_from_slice(&(!crc).to_be_bytes());
            c
        }
        let mut ihdr = 60_000u32.to_be_bytes().to_vec();
        ihdr.extend_from_slice(&60_000u32.to_be_bytes());
        ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        png.extend(chunk(b"IHDR", &ihdr));
        png.extend(chunk(b"IDAT", &miniz_oxide::deflate::compress_to_vec_zlib(&[0; 64], 9)));
        png.extend(chunk(b"IEND", &[]));
        let err = ImageStore::new().load_bytes(&png).unwrap_err().to_string();
        assert!(err.contains("larger than"), "{err}");
    }

    #[cfg(unix)]
    #[test]
    fn refuses_links_out_of_the_resource_directory() {
        let dir = std::env::temp_dir().join(format!("ferroreport-link-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let link = dir.join("passwd.png");
        let _ = std::fs::remove_file(&link);
        std::os::unix::fs::symlink("/etc/passwd", &link).unwrap();
        let mut store = ImageStore::new();
        store.add_root(&dir);
        let err = store.load_location("passwd.png").unwrap_err().to_string();
        assert!(err.contains("outside"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn decodes_a_data_uri_png() {
        // 1x1 transparent PNG.
        let uri = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";
        let img = ImageStore::new().load_location(uri).unwrap();
        assert_eq!((img.width, img.height), (1, 1));
    }
}
