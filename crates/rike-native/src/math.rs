//! Formulas: TeX -> Typst math (`mitex`) -> page (`typst`) -> PNG.
//!
//! `TextView` finds the formulas; this module only turns one into an image.
//! Nothing here touches the disk or the network, and nothing here is NSH.

use std::cell::RefCell;
use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;
use std::sync::OnceLock;

use typst::diag::{FileError, FileResult};
use typst::foundations::{Bytes, Datetime, Duration};
use typst::layout::{Abs, Frame, FrameItem};
use typst::syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World};
use typst_layout::PagedDocument;
use typst_render::RenderOptions;

/// Text size of a formula, in points. One point is one logical pixel here.
/// It is the body size of NSH's `TextView` (`BODY_SIZE`, private there), so a
/// formula matches the text around it.
pub const FONT_PT: f32 = 15.0;
/// Pixels rendered per logical pixel; the image is shown at its logical size.
const PIXELS_PER_PT: f32 = 2.0;
/// The cache is dropped when its images add up to more than this.
const CACHE_BYTES: usize = 32 << 20;

/// `mitex` is written for older Typst: its output names functions that its
/// own Typst package defines. They are defined here.
const PRELUDE: &str = r#"
#let zws = sym.zws
#let mitexdisplay(body) = math.display(body)
#let mitexinline(body) = math.inline(body)
#let textmath(body) = body
#let diff = sym.partial
#let mitexmathbf(body) = math.bold(math.upright(body))
#let mitexsqrt(..args) = {
  let a = args.pos()
  if a.len() == 1 { math.sqrt(a.at(0)) } else { math.root(a.at(0), a.at(1)) }
}
#let operatorname(body) = math.op(math.upright(body))
#let aligned(body) = body
#let mitexarray(arg0: none, ..rows) = math.mat(delim: none, ..rows)
#let varGamma = math.italic(sym.Gamma)
#let varDelta = math.italic(sym.Delta)
#let varTheta = math.italic(sym.Theta)
#let varLambda = math.italic(sym.Lambda)
#let varXi = math.italic(sym.Xi)
#let varPi = math.italic(sym.Pi)
#let varSigma = math.italic(sym.Sigma)
#let varUpsilon = math.italic(sym.Upsilon)
#let varPhi = math.italic(sym.Phi)
#let varPsi = math.italic(sym.Psi)
#let varOmega = math.italic(sym.Omega)
"#;

/// Symbols `mitex` spells the way older Typst did, as `(old, new)` over the
/// converted text.
const RENAMED: &[(&str, &str)] = &[("dot.circle", "⊙")];

/// A formula rendered to PNG.
pub struct Rendered {
    pub png: Vec<u8>,
    /// Size of the image in pixels.
    pub pixels: (u32, u32),
    /// Layout size in logical pixels.
    pub width: f32,
    pub height: f32,
    /// Distance from the top to the baseline, in logical pixels.
    pub baseline: f32,
}

/// A formula ready to show: an image URI and where it sits.
pub struct Formula {
    pub uri: String,
    pub width: f32,
    pub height: f32,
    pub baseline: f32,
}

struct Shared {
    library: LazyHash<Library>,
    book: LazyHash<FontBook>,
    fonts: Vec<Font>,
}

fn shared() -> &'static Shared {
    static SHARED: OnceLock<Shared> = OnceLock::new();
    SHARED.get_or_init(|| {
        let fonts: Vec<Font> = typst_assets::fonts()
            .flat_map(|data| Font::iter(Bytes::new(data)))
            .collect();
        Shared {
            library: LazyHash::new(Library::default()),
            book: LazyHash::new(FontBook::from_fonts(&fonts)),
            fonts,
        }
    })
}

struct Job {
    shared: &'static Shared,
    main: Source,
}

impl World for Job {
    fn library(&self) -> &LazyHash<Library> {
        &self.shared.library
    }

    fn book(&self) -> &LazyHash<FontBook> {
        &self.shared.book
    }

    fn main(&self) -> FileId {
        self.main.id()
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        if id == self.main.id() {
            Ok(self.main.clone())
        } else {
            Err(FileError::NotFound(std::path::PathBuf::new()))
        }
    }

    fn file(&self, _id: FileId) -> FileResult<Bytes> {
        Err(FileError::NotFound(std::path::PathBuf::new()))
    }

    fn font(&self, index: usize) -> Option<Font> {
        self.shared.fonts.get(index).cloned()
    }

    fn today(&self, _offset: Option<Duration>) -> Option<Datetime> {
        None
    }
}

/// Loads the fonts and warms Typst's caches on another thread, so the first
/// formula on screen does not stall the UI.
pub fn prewarm() {
    std::thread::spawn(|| {
        let _ = compile(r"\frac{a}{b} + \sqrt{x}", true, false);
    });
}

/// TeX math as Typst math, without the surrounding `$`.
pub fn to_typst(tex: &str) -> Result<String, String> {
    let mut math = mitex::convert_math(tex, None)?;
    for (old, new) in RENAMED {
        math = math.replace(old, new);
    }
    Ok(math)
}

/// The source of a page that fits the formula and is transparent. `$ x $`
/// (spaces inside) is Typst's display math, `$x$` its inline math.
fn document(math: &str, display: bool, dark: bool) -> String {
    let color = if dark { "#f4f4f5" } else { "#18181b" };
    let body = if display {
        format!("$ {math} $")
    } else {
        format!("#box(width: 0pt, text(size: {MARKER_PT}pt, fill: rgb(\"#00000000\"))[H])${math}$")
    };
    format!(
        "{PRELUDE}\n#set page(width: auto, height: auto, margin: (x: 1pt, y: 2pt), fill: none)\n\
         #set text(size: {FONT_PT}pt, fill: rgb(\"{color}\"), top-edge: \"bounds\", bottom-edge: \"bounds\")\n{body}\n"
    )
}

/// Runs `f`, turning a panic (in `mitex` or Typst) into an error: a formula
/// that cannot be drawn must not take the app, and its unsaved text, down.
fn guarded<T>(f: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or_else(|_| Err("the renderer panicked".to_owned()))
}

/// Size of the invisible marker text put on the formula's line. Typst puts
/// no baseline on a page of flattened items, and a fraction has no text on
/// its baseline, so the marker's own baseline is read instead.
const MARKER_PT: f64 = 7.0;

/// Distance from the top of the page to the marker's baseline.
fn marker_baseline(frame: &Frame) -> Option<Abs> {
    fn walk(frame: &Frame, top: Abs) -> Option<Abs> {
        for (pos, item) in frame.items() {
            match item {
                FrameItem::Text(text) if text.size == Abs::pt(MARKER_PT) => {
                    return Some(top + pos.y);
                }
                FrameItem::Group(group) => {
                    if let Some(found) = walk(&group.frame, top + pos.y) {
                        return Some(found);
                    }
                }
                _ => {}
            }
        }
        None
    }
    walk(frame, Abs::zero())
}

/// Renders `tex` (TeX math) as a transparent PNG; `dark` picks the text
/// colour. The error is a message for a log or a test.
pub fn compile(tex: &str, display: bool, dark: bool) -> Result<Rendered, String> {
    guarded(|| {
        let text = document(&to_typst(tex)?, display, dark);
        let id = FileId::new(RootedPath::new(
            VirtualRoot::Project,
            VirtualPath::new("/main.typ").map_err(|e| e.to_string())?,
        ));
        let job = Job {
            shared: shared(),
            main: Source::new(id, text),
        };
        let warned = typst::compile::<PagedDocument>(&job);
        let result = warned.output.map_err(|errors| {
            errors
                .iter()
                .map(|e| e.message.to_string())
                .collect::<Vec<_>>()
                .join("; ")
        });
        // Typst memoises layout between compiles; drop what has gone stale.
        typst::comemo::evict(10);
        let document = result?;
        let page = document.pages().first().ok_or("no page")?;
        let pixmap = typst_render::render(
            page,
            &RenderOptions {
                pixel_per_pt: f64::from(PIXELS_PER_PT).into(),
                ..RenderOptions::default()
            },
        );
        let png = pixmap.encode_png().map_err(|e| e.to_string())?;
        let (w, h) = (pixmap.width(), pixmap.height());
        let height = h as f32 / PIXELS_PER_PT;
        let baseline = marker_baseline(&page.frame)
            .map_or(height, |b| b.to_pt() as f32)
            .clamp(0.0, height);
        Ok(Rendered {
            png,
            pixels: (w, h),
            width: w as f32 / PIXELS_PER_PT,
            height,
            baseline,
        })
    })
}

#[derive(Default)]
struct Cache {
    map: HashMap<(String, bool, bool), Option<Rc<Formula>>>,
    bytes: usize,
}

thread_local! {
    static CACHE: RefCell<Cache> = RefCell::new(Cache::default());
}

/// The formula as an image, or `None` when it cannot be drawn (the caller
/// shows the TeX). Results, failures included, are cached.
pub fn render(tex: &str, display: bool, dark: bool) -> Option<Rc<Formula>> {
    let key = (tex.to_owned(), display, dark);
    if let Some(hit) = CACHE.with(|c| c.borrow().map.get(&key).cloned()) {
        return hit;
    }
    let formula = compile(tex, display, dark).ok().map(|r| {
        Rc::new(Formula {
            uri: format!("data:image/png;base64,{}", base64(&r.png)),
            width: r.width,
            height: r.height,
            baseline: r.baseline,
        })
    });
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if c.bytes > CACHE_BYTES {
            c.map.clear();
            c.bytes = 0;
        }
        c.bytes += key.0.len() + formula.as_ref().map_or(0, |f| f.uri.len());
        c.map.insert(key, formula.clone());
    });
    formula
}

fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for (i, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            if i <= chunk.len() {
                out.push(TABLE[(n >> shift & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_standard_alphabet_and_padding() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
    }

    #[test]
    fn renders_a_formula_to_png() {
        let r = compile(r"\frac{a}{b}+\sqrt{x}", false, false).unwrap();
        assert!(r.png.starts_with(&[0x89, b'P', b'N', b'G']));
        assert!(r.width > 10.0 && r.height > 10.0);
        assert!(r.baseline > 0.0 && r.baseline <= r.height);
    }

    #[test]
    fn names_mitex_writes_for_older_typst_still_compile() {
        for tex in [
            r"\frac{\partial\mathbf u}{\partial t}",
            r"\sqrt{x}+\operatorname{rank}(X)",
            r"A \circ B \odot C",
            r"\varPhi",
            r"\begin{aligned} a &= b \\ c &= d \end{aligned}",
            r"\left\{\begin{array}{ll} 1 & x>0 \\ 0 & \text{else} \end{array}\right.",
        ] {
            compile(tex, true, false).unwrap_or_else(|e| panic!("{tex}: {e}"));
        }
    }

    #[test]
    fn display_is_larger_than_inline_for_a_sum() {
        let tex = r"\sum_{i=1}^{n} i";
        let inline = compile(tex, false, false).unwrap();
        let display = compile(tex, true, false).unwrap();
        assert!(display.height > inline.height);
    }

    #[test]
    fn baseline_follows_the_glyphs_not_the_box() {
        let flat = compile("x", false, false).unwrap();
        let sub = compile("x_i", false, false).unwrap();
        // A subscript hangs below the baseline: the box grows downwards.
        assert!(sub.height > flat.height);
        assert!(sub.height - sub.baseline > flat.height - flat.baseline);
        // The baseline of a lone letter sits near the bottom of its box.
        assert!(flat.height - flat.baseline < flat.height / 2.0);
    }

    #[test]
    fn a_fraction_sits_on_its_axis_not_its_bottom() {
        let frac = compile(r"\frac{a}{b}", false, false).unwrap();
        let below = frac.height - frac.baseline;
        // The denominator hangs well below the baseline; the numerator rises above.
        assert!(below > 3.0, "{} {}", frac.height, frac.baseline);
        assert!(frac.baseline > below);
    }

    #[test]
    fn colour_follows_the_theme() {
        let light = compile("x", false, false).unwrap();
        let dark = compile("x", false, true).unwrap();
        assert_ne!(light.png, dark.png);
        assert_eq!(light.pixels, dark.pixels);
    }

    #[test]
    fn a_panic_becomes_an_error() {
        let result: Result<(), String> = guarded(|| panic!("boom"));
        assert!(result.is_err());
    }

    #[test]
    fn results_are_cached_failures_included() {
        let a = render("a+b", false, false).unwrap();
        let b = render("a+b", false, false).unwrap();
        assert!(Rc::ptr_eq(&a, &b));
        assert!(render(r"\begin{", false, false).is_none());
        assert!(render(r"\begin{", false, false).is_none());
    }
}
