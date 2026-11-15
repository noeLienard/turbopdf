//! HTML/CSS previews, drawn from the same [`Layout`] as the PDF.
//!
//! Every page becomes a `<section class="turbopdf-page">` of the page's size in points, holding absolutely
//! positioned elements: boxes are `<div>`s with CSS backgrounds and borders, text is real text in the
//! document's own fonts (embedded once as `@font-face`), images are CSS backgrounds, links are `<a>`.
//! Text can be selected, copied and searched, links work (internal ones jump to their target), and the
//! result is the same whatever the source was (HTML, Markdown, React or a tree): it is the PDF's layout.
//!
//! Text is placed word by word at the positions the engine computed, so the browser's own shaping can
//! never push a line out of place by more than a fraction of a word.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use std::collections::BTreeSet;
use std::fmt::Write as _;
use turbopdf_fonts::FontDb;
use turbopdf_layout::{DrawOp, ImageKind, Layout, StructRole, Tag, Visual};
use turbopdf_text::{Rgba, Run};
use turbopdf_tree::{BorderStyle, ObjectFit};

mod woff;

/// One page of the preview.
#[derive(Clone, Debug, PartialEq)]
pub struct HtmlPage {
    /// Page size in points.
    pub width: f32,
    pub height: f32,
    /// `<section class="turbopdf-page">…</section>`; needs [`HtmlPreview::style`] in the same document.
    pub html: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HtmlPreview {
    /// CSS shared by all pages: base rules, the fonts used (`@font-face`) and the images.
    pub style: String,
    pub pages: Vec<HtmlPage>,
}

impl HtmlPreview {
    /// A complete HTML document: the style and every page, one under the other.
    pub fn document(&self, title: &str, lang: Option<&str>) -> String {
        let mut s = String::with_capacity(
            self.style.len() + self.pages.iter().map(|p| p.html.len()).sum::<usize>() + 512,
        );
        let _ = write!(
            s,
            "<!DOCTYPE html><html{}><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width\"><title>{}</title><style>body{{margin:0;padding:24px 0;background:#e5e7eb}}.turbopdf-page{{margin:0 auto 24px;box-shadow:0 2px 12px rgba(0,0,0,.15)}}{}</style></head><body>",
            lang.map(|l| format!(" lang=\"{}\"", esc(l)))
                .unwrap_or_default(),
            esc(title),
            self.style
        );
        for p in &self.pages {
            s.push_str(&p.html);
        }
        s.push_str("</body></html>");
        s
    }
}

const BASE: &str = ".turbopdf-page{position:relative;overflow:hidden;background:#fff;color:#000;box-sizing:border-box;text-align:left;font-kerning:normal;font-synthesis:none;-webkit-font-smoothing:antialiased;text-rendering:geometricPrecision}\
.turbopdf-page>*,.turbopdf-page .pr>*{position:absolute;margin:0;padding:0;box-sizing:border-box}\
.turbopdf-page .pw{white-space:pre;line-height:normal;font-style:normal;font-weight:400;letter-spacing:0;word-spacing:0;text-transform:none}\
.turbopdf-page .pr{width:0;height:0;transform-origin:0 0}\
.turbopdf-page .pi{background-repeat:no-repeat;background-position:center}\
.turbopdf-page .pl{display:block;background:transparent;text-decoration:none}\
.turbopdf-page .pa{width:0;height:0}";

/// Layout → HTML pages. Infallible: everything was decoded and measured by the layout.
pub fn render(layout: &Layout, db: &FontDb) -> HtmlPreview {
    let mut style = String::from(BASE);

    // fonts: only the faces used, each embedded once, with the engine's metrics so that every browser
    // puts the baseline where the engine did (`ascent-override` etc.)
    // and only the glyphs used, compressed (WOFF): a few dozen KB instead of ~550 KB per face
    let mut faces: std::collections::BTreeMap<usize, BTreeSet<u16>> = Default::default();
    for p in &layout.pages {
        for op in &p.ops {
            if let DrawOp::Text { run, .. } | DrawOp::Rotated { run, .. } = op {
                faces
                    .entry(run.face)
                    .or_default()
                    .extend(run.glyphs.iter().map(|g| g.id));
            }
        }
    }
    for (&f, glyphs) in &faces {
        let face = db.face(f);
        let light = woff::light_woff(face.data.as_slice(), glyphs);
        let (mime, format, bytes) = match &light {
            Some(w) => ("font/woff", "woff", w.as_slice()),
            None if face.data.starts_with(b"OTTO") => {
                ("font/otf", "opentype", face.data.as_slice())
            }
            None => ("font/ttf", "truetype", face.data.as_slice()),
        };
        let pct = |v: f32| v / face.units_per_em * 100.0;
        let _ = write!(
            style,
            "@font-face{{font-family:\"turbopdf-{f}\";src:url(data:{mime};base64,{}) format(\"{format}\");ascent-override:{:.4}%;descent-override:{:.4}%;line-gap-override:0%;font-display:block}}.turbopdf-page .pf{f}{{font-family:\"turbopdf-{f}\"}}",
            B64.encode(bytes),
            pct(face.ascender),
            pct(-face.descender),
        );
    }

    // images: each one once, as a class
    let mut used = BTreeSet::new();
    for p in &layout.pages {
        for op in &p.ops {
            if let DrawOp::Image { image, .. } = op {
                used.insert(*image);
            }
        }
    }
    // the same file used twice is decoded twice by the layout: embed its bytes once
    let mut class_of: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
    let mut seen: std::collections::HashMap<&[u8], usize> = std::collections::HashMap::new();
    for &i in &used {
        let data = &layout.images[i];
        if let Some(&first) = seen.get(data.bytes.as_slice()) {
            class_of.insert(i, first);
            continue;
        }
        seen.insert(data.bytes.as_slice(), i);
        class_of.insert(i, i);
        let mime = match data.kind {
            ImageKind::Png => "image/png",
            ImageKind::Jpeg => "image/jpeg",
            ImageKind::Webp => "image/webp",
            ImageKind::Gif => "image/gif",
            ImageKind::Svg => "image/svg+xml",
        };
        let _ = write!(
            style,
            ".turbopdf-page .pi{i}{{background-image:url(data:{mime};base64,{})}}",
            B64.encode(data.bytes.as_slice())
        );
    }

    let pages = layout
        .pages
        .iter()
        .enumerate()
        .map(|(n, page)| render_page(layout, db, n, page, &class_of))
        .collect();
    HtmlPreview { style, pages }
}

fn render_page(
    layout: &Layout,
    db: &FontDb,
    n: usize,
    page: &turbopdf_layout::PageLayout,
    image_class: &std::collections::HashMap<usize, usize>,
) -> HtmlPage {
    let mut h = String::new();
    let _ = write!(
        h,
        "<section class=\"turbopdf-page\" style=\"width:{}pt;height:{}pt\" data-page=\"{}\" aria-label=\"Page {}\">",
        num(page.width),
        num(page.height),
        n + 1,
        n + 1
    );

    // link targets on this page, before the content so that they are found first
    for (name, a) in &layout.anchors {
        if a.page == n {
            let _ = write!(
                h,
                "<span class=\"pa\" id=\"{}\" style=\"left:{}pt;top:{}pt\"></span>",
                esc(&anchor_id(name)),
                num(a.x),
                num(a.y)
            );
        }
    }

    for (i, op) in page.ops.iter().enumerate() {
        let tag = page.tags.get(i);
        match op {
            DrawOp::Box {
                x,
                y,
                w,
                h: bh,
                visual,
                sides,
            } => draw_box(&mut h, *x, *y, *w, *bh, visual, *sides),
            DrawOp::Text { x, baseline, run } => words(&mut h, db, *x, *baseline, run),
            DrawOp::Rotated {
                cx,
                cy,
                angle,
                x,
                baseline,
                run,
            } => {
                let _ = write!(
                    h,
                    "<div class=\"pr\" aria-hidden=\"true\" style=\"left:{}pt;top:{}pt;transform:rotate({}deg)\">",
                    num(*cx),
                    num(*cy),
                    num(-angle)
                );
                words(&mut h, db, *x, *baseline, run);
                h.push_str("</div>");
            }
            DrawOp::Image {
                x,
                y,
                w,
                h: ih,
                image,
                fit,
            } => {
                let (nw, nh) = layout.images[*image].natural;
                let size = match fit {
                    ObjectFit::Fill => "100% 100%".to_string(),
                    ObjectFit::Contain => "contain".to_string(),
                    ObjectFit::Cover => "cover".to_string(),
                    ObjectFit::None => format!("{}pt {}pt", num(nw), num(nh)),
                };
                let alt = match tag {
                    Some(Tag::Content { elem, .. }) => {
                        match layout.structure.get(*elem as usize).map(|e| &e.role) {
                            Some(StructRole::Figure(Some(alt))) => Some(alt.as_str()),
                            _ => None,
                        }
                    }
                    _ => None,
                };
                let a11y = match alt {
                    Some(alt) => format!("role=\"img\" aria-label=\"{}\"", esc(alt)),
                    None => "aria-hidden=\"true\"".to_string(),
                };
                let class = image_class.get(image).copied().unwrap_or(*image);
                let _ = write!(
                    h,
                    "<div class=\"pi pi{class}\" {a11y} style=\"left:{}pt;top:{}pt;width:{}pt;height:{}pt;background-size:{size}\"></div>",
                    num(*x),
                    num(*y),
                    num(*w),
                    num(*ih)
                );
            }
        }
    }

    // links last: on top of everything, like PDF annotations
    for l in &page.links {
        let target = &layout.links[l.link as usize];
        let (href, external) = match target.strip_prefix('#') {
            Some(id) => (format!("#{}", anchor_id(id)), false),
            None if turbopdf_layout::is_safe_external_link(target) => {
                (target.trim().to_string(), true)
            }
            None => continue,
        };
        let _ = write!(
            h,
            "<a class=\"pl\" href=\"{}\"{} style=\"left:{}pt;top:{}pt;width:{}pt;height:{}pt\"></a>",
            esc(&href),
            if external {
                " target=\"_blank\" rel=\"noopener noreferrer\""
            } else {
                ""
            },
            num(l.x),
            num(l.y),
            num(l.w),
            num(l.h)
        );
    }
    h.push_str("</section>");
    HtmlPage {
        width: page.width,
        height: page.height,
        html: h,
    }
}

/// Ids of link targets are prefixed: the preview lives inside someone else's page.
fn anchor_id(name: &str) -> String {
    format!("turbopdf-{name}")
}

fn draw_box(h: &mut String, x: f32, y: f32, w: f32, bh: f32, v: &Visual, sides: [bool; 4]) {
    if w <= 0.0 || bh <= 0.0 || v.is_empty() {
        return;
    }
    let _ = write!(
        h,
        "<div aria-hidden=\"true\" style=\"left:{}pt;top:{}pt;width:{}pt;height:{}pt",
        num(x),
        num(y),
        num(w),
        num(bh)
    );
    if let Some(bg) = v.background {
        let _ = write!(h, ";background:{}", color(bg));
    }
    if v.radius > 0.0 {
        let _ = write!(h, ";border-radius:{}pt", num(v.radius));
    }
    if let Some(b) = v.border {
        let style = match b.style {
            BorderStyle::Solid => "solid",
            BorderStyle::Dashed => "dashed",
            BorderStyle::Dotted => "dotted",
        };
        let spec = format!("{}pt {style} {}", num(b.width), color(b.color));
        if sides.iter().all(|&s| s) {
            let _ = write!(h, ";border:{spec}");
        } else {
            for (on, side) in sides.iter().zip(["top", "right", "bottom", "left"]) {
                if *on {
                    let _ = write!(h, ";border-{side}:{spec}");
                }
            }
        }
    }
    h.push_str("\"></div>");
}

/// A run, one `<span>` per word, each at the x the engine computed. The trailing space stays in its
/// word, so copied text reads normally.
fn words(h: &mut String, db: &FontDb, x: f32, baseline: f32, run: &Run) {
    if run.glyphs.is_empty() || run.color[3] == 0 {
        return;
    }
    let face = db.face(run.face);
    let top = baseline - run.size * face.ascender / face.units_per_em;
    let mut pen = x;
    let mut start = pen;
    let mut range: Option<(usize, usize)> = None;
    let flush = |h: &mut String, start: f32, range: &mut Option<(usize, usize)>| {
        if let Some((a, b)) = range.take()
            && let Some(text) = run.text.get(a..b)
            && !text.trim().is_empty()
        {
            let _ = write!(
                h,
                "<span class=\"pw pf{}\" style=\"left:{}pt;top:{}pt;font-size:{}pt;color:{}\">{}</span>",
                run.face,
                num(start),
                num(top),
                num(run.size),
                color(run.color),
                esc(text)
            );
        }
    };
    for g in &run.glyphs {
        if range.is_none() {
            start = pen;
        }
        let r = range.get_or_insert((g.range.start, g.range.end));
        r.0 = r.0.min(g.range.start);
        r.1 = r.1.max(g.range.end);
        pen += g.advance_em * run.size;
        let ends_word = run
            .text
            .get(g.range.clone())
            .is_some_and(|t| t.chars().all(char::is_whitespace));
        if ends_word {
            flush(h, start, &mut range);
        }
    }
    flush(h, start, &mut range);
}

fn color(c: Rgba) -> String {
    if c[3] == 255 {
        format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
    } else {
        format!("#{:02x}{:02x}{:02x}{:02x}", c[0], c[1], c[2], c[3])
    }
}

/// Points with at most 2 decimals, no trailing zeros.
fn num(v: f32) -> String {
    let s = format!("{:.2}", v);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}

fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&#39;"),
            _ => o.push(c),
        }
    }
    o
}
