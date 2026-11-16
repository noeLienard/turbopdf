//! Font database.
//!
//! * Inter (Regular, SemiBold, Bold, Italic, Bold Italic) is **embedded in the binary**: the same tree
//!   renders identically on every machine, and it is the final fallback.
//! * Extra fonts come from the tree (`fonts`) as bytes (inline base64, or `file` resolved by the host
//!   through [`ResourceLoader`]). The core never touches the filesystem or the network.
//! * Only the glyphs actually used end up in the PDF (subsetting is done by the PDF writer).
//!
//! Iteration order is always insertion order (`Vec`), never a hash map: determinism.

use std::sync::Arc;

pub use rustybuzz::Face as ShapeFace;

/// Resolves `{ "file": "…" }` sources. Implemented by the Node binding; absent in WebAssembly.
pub trait ResourceLoader: Send + Sync {
    fn load(&self, path: &str) -> Result<Vec<u8>, String>;

    /// Bytes handed over next to the tree (`{ "buffer": n }` sources).
    fn buffer(&self, index: u32) -> Result<Vec<u8>, String> {
        Err(format!("no buffer {index} was passed with the document"))
    }
}

/// Loader that always fails: the default when the host provides none.
pub struct NoLoader;
impl ResourceLoader for NoLoader {
    fn load(&self, path: &str) -> Result<Vec<u8>, String> {
        Err(format!(
            "{path:?}: no baseDir was given, so files are not read. Pass the bytes instead (fonts: data, images: data: URLs)"
        ))
    }
}

/// Index of a face inside a [`FontDb`].
pub type FaceId = usize;

/// One font file, parsed **once** (parsing 400 KB of tables for every word was the first bottleneck).
pub struct Face {
    pub family: String,
    pub weight: u16,
    pub italic: bool,
    pub units_per_em: f32,
    pub ascender: f32,
    pub descender: f32,
    pub line_gap: f32,
    pub x_height: f32,
    // Field order matters: `shaper` borrows from `data` and must be dropped first.
    shaper: ShapeFace<'static>,
    pub data: Arc<Vec<u8>>,
}

impl Face {
    /// The parsed face, ready for shaping and outline extraction.
    pub fn shaper(&self) -> &ShapeFace<'_> {
        &self.shaper
    }
}

#[derive(Debug, thiserror::Error)]
pub enum FontError {
    #[error("font {family:?}: the data is not a valid TrueType/OpenType font")]
    Invalid { family: String },
    #[error("font {family:?}: {reason}")]
    Load { family: String, reason: String },
}

const INTER: &[(&str, u16, bool, &[u8])] = &[
    (
        "Inter",
        400,
        false,
        include_bytes!("../assets/Inter-Regular.ttf"),
    ),
    (
        "Inter",
        600,
        false,
        include_bytes!("../assets/Inter-SemiBold.ttf"),
    ),
    (
        "Inter",
        700,
        false,
        include_bytes!("../assets/Inter-Bold.ttf"),
    ),
    (
        "Inter",
        400,
        true,
        include_bytes!("../assets/Inter-Italic.ttf"),
    ),
    (
        "Inter",
        700,
        true,
        include_bytes!("../assets/Inter-BoldItalic.ttf"),
    ),
];

/// All fonts known to one render. Cloning is cheap (faces are shared).
#[derive(Clone)]
pub struct FontDb {
    faces: Vec<Arc<Face>>,
}

impl Default for FontDb {
    fn default() -> Self {
        Self::with_embedded()
    }
}

impl FontDb {
    /// A database holding only the embedded Inter faces.
    pub fn with_embedded() -> Self {
        let mut db = FontDb { faces: Vec::new() };
        for (family, weight, italic, bytes) in INTER {
            db.add(family, *weight, *italic, bytes.to_vec())
                .expect("embedded Inter is valid");
        }
        db
    }

    /// Add a font. Later additions win ties over earlier ones with the same key.
    pub fn add(
        &mut self,
        family: &str,
        weight: u16,
        italic: bool,
        data: Vec<u8>,
    ) -> Result<FaceId, FontError> {
        let data = Arc::new(data);
        // SAFETY: the slice points into the heap buffer of the `Arc<Vec<u8>>` stored next to the parsed
        // face in the same struct. The buffer is never mutated or moved, and `shaper` is declared before
        // `data`, so it is dropped first. The 'static lifetime never escapes: `shaper()` re-borrows it
        // for the lifetime of `&Face`.
        let slice: &'static [u8] = unsafe { std::slice::from_raw_parts(data.as_ptr(), data.len()) };
        let probe = ShapeFace::from_slice(slice, 0).ok_or_else(|| FontError::Invalid {
            family: family.to_string(),
        })?;
        let upem = probe.units_per_em() as f32;
        let face = Face {
            family: family.to_string(),
            weight,
            italic,
            units_per_em: upem,
            ascender: probe.ascender() as f32,
            descender: probe.descender() as f32,
            line_gap: probe.line_gap() as f32,
            x_height: probe.x_height().map(|v| v as f32).unwrap_or(upem * 0.5),
            shaper: probe,
            data,
        };
        self.faces.push(Arc::new(face));
        Ok(self.faces.len() - 1)
    }

    pub fn face(&self, id: FaceId) -> &Face {
        &self.faces[id]
    }

    pub fn len(&self) -> usize {
        self.faces.len()
    }

    pub fn is_empty(&self) -> bool {
        self.faces.is_empty()
    }

    /// The families of a CSS-like list (`"Brand", Georgia, sans-serif`), quotes removed. Generic
    /// sans-serif names mean the built-in Inter.
    fn families(list: &str) -> impl Iterator<Item = &str> {
        list.split(',')
            .map(|f| f.trim().trim_matches(['"', '\'']).trim())
            .filter(|f| !f.is_empty())
            .map(|f| match f.to_ascii_lowercase().as_str() {
                "sans-serif" | "system-ui" | "ui-sans-serif" | "-apple-system" => "Inter",
                _ => f,
            })
    }

    /// The loaded families of `list`, in order (each once).
    fn known<'a>(&self, list: &'a str) -> Vec<&'a str> {
        let mut out: Vec<&str> = Vec::new();
        for f in Self::families(list) {
            if self.has_one_family(f) && !out.iter().any(|o| o.eq_ignore_ascii_case(f)) {
                out.push(f);
            }
        }
        out
    }

    /// Best face for a request. `family` may be a list (`Brand, Georgia, sans-serif`): the first loaded
    /// family is used. Then italic match → nearest weight. If no family of the list is loaded, Inter
    /// is used (final fallback).
    pub fn select(&self, family: &str, weight: u16, italic: bool) -> FaceId {
        let name = self.known(family).first().copied().unwrap_or("Inter");
        self.select_in(name, weight, italic)
    }

    /// The face of the next loaded family of the list, for characters the first one does not have
    /// (as browsers do). `None` when the list has a single loaded family.
    pub fn select_fallback(&self, family: &str, weight: u16, italic: bool) -> Option<FaceId> {
        self.known(family)
            .get(1)
            .map(|name| self.select_in(name, weight, italic))
    }

    fn select_in(&self, family: &str, weight: u16, italic: bool) -> FaceId {
        let in_family = |f: &str| -> Vec<FaceId> {
            (0..self.faces.len())
                .filter(|&i| self.faces[i].family.eq_ignore_ascii_case(f))
                .collect()
        };
        let mut candidates = in_family(family);
        if candidates.is_empty() {
            candidates = in_family("Inter");
        }
        let same_style: Vec<FaceId> = candidates
            .iter()
            .copied()
            .filter(|&i| self.faces[i].italic == italic)
            .collect();
        let pool = if same_style.is_empty() {
            candidates
        } else {
            same_style
        };
        // nearest weight; on a tie prefer the one added last, then the heavier one
        *pool
            .iter()
            .max_by_key(|&&i| {
                let d = (self.faces[i].weight as i32 - weight as i32).abs();
                (-d, i as i32)
            })
            .expect("pool is never empty")
    }

    /// Whether `family` (or one family of a list) is loaded (used to report a fallback).
    pub fn has_family(&self, family: &str) -> bool {
        !self.known(family).is_empty()
    }

    fn has_one_family(&self, family: &str) -> bool {
        self.faces
            .iter()
            .any(|f| f.family.eq_ignore_ascii_case(family))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_inter_is_available() {
        let db = FontDb::with_embedded();
        assert_eq!(db.len(), 5);
        let f = db.face(db.select("Inter", 400, false));
        assert_eq!((f.weight, f.italic), (400, false));
        assert!(f.units_per_em > 0.0 && f.ascender > 0.0 && f.descender < 0.0);
    }

    #[test]
    fn selects_nearest_weight_and_style() {
        let db = FontDb::with_embedded();
        let pick = |w, i| {
            let f = db.face(db.select("Inter", w, i));
            (f.weight, f.italic)
        };
        assert_eq!(pick(700, false), (700, false));
        assert_eq!(pick(500, false), (600, false)); // 500 is closer to 600 than to 400
        assert_eq!(pick(300, false), (400, false));
        assert_eq!(pick(900, false), (700, false));
        assert_eq!(pick(400, true), (400, true));
        assert_eq!(pick(600, true), (700, true));
    }

    #[test]
    fn unknown_family_falls_back_to_inter() {
        let db = FontDb::with_embedded();
        assert_eq!(
            db.face(db.select("Comic TurboPDF", 400, false)).family,
            "Inter"
        );
        assert!(!db.has_family("Comic TurboPDF"));
        assert!(db.has_family("inter"));
    }

    #[test]
    fn user_fonts_take_priority_for_their_family() {
        let mut db = FontDb::with_embedded();
        let data = include_bytes!("../assets/Inter-Regular.ttf").to_vec();
        let id = db.add("Brand", 400, false, data).unwrap();
        assert_eq!(db.select("brand", 400, false), id);
        assert_eq!(db.select("Brand", 700, true), id); // only face of the family
    }

    #[test]
    fn family_lists_use_the_first_loaded_family_and_keep_the_next_as_fallback() {
        let mut db = FontDb::with_embedded();
        let data = include_bytes!("../assets/Inter-Regular.ttf").to_vec();
        let brand = db.add("Brand", 400, false, data).unwrap();
        assert_eq!(db.select("'Missing', \"Brand\", serif", 400, false), brand);
        assert_eq!(db.select_fallback("Missing, Brand", 400, false), None);
        let inter = db.select("Inter", 400, false);
        assert_eq!(db.select("Georgia, sans-serif", 400, false), inter);
        assert_eq!(db.select("Inter, Brand", 400, false), inter);
        assert_eq!(db.select_fallback("Inter, Brand", 400, false), Some(brand));
        assert!(db.has_family("Nope, Brand"));
        assert!(!db.has_family("Nope, Georgia"));
    }

    #[test]
    fn invalid_font_data_is_rejected() {
        let mut db = FontDb::with_embedded();
        assert!(matches!(
            db.add("Bad", 400, false, vec![1, 2, 3]),
            Err(FontError::Invalid { .. })
        ));
    }
}
