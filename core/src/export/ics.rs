//! ICS forms: fill FEMA's official AcroForm PDFs (bundled in `assets/ics`, see docs/ICS_FIELD_MAP.md).
//! The forms default to Helvetica with no `NeedAppearances`, so every filled field gets its own
//! appearance stream drawn with the embedded DejaVu Sans (Unicode), and its `/DA` names that font so
//! the result stays editable. Signature fields are never touched. Text that does not fit a field is
//! never dropped: it is returned as overflow for the caller to continue on a continuation page.
//!
//! [`FormBuilder`] assembles one output document from copies of template pages. Each copy's fields
//! get a suffix so copies hold independent values, and all copies share one embedded font.

use super::pdf::{FONT_DATA, FontMetrics, wrap};
use crate::error::{AppError, AppResult};
use lopdf::{Dictionary, Document, Object, ObjectId, Stream, StringFormat};
use std::collections::{BTreeMap, HashMap};

pub static ICS_213: &[u8] = include_bytes!("../../assets/ics/ICS-213-v3.pdf");
pub static ICS_214: &[u8] = include_bytes!("../../assets/ics/ICS-214-v3.1.pdf");

const FONT_RES: &str = "EmDejaVu";
const MAX_SIZE: f32 = 9.0;
const MIN_SIZE: f32 = 6.0;
const MIN_MULTILINE: f32 = 7.0;
const PAD: f32 = 2.0;
const LINE: f32 = 1.15;
const LETTER: (f32, f32) = (612.0, 792.0);

fn err(e: lopdf::Error) -> AppError {
    AppError::internal(format!("ICS form template: {e}"))
}

fn resolve<'a>(doc: &'a Document, o: &'a Object) -> AppResult<&'a Object> {
    match o {
        Object::Reference(id) => doc.get_object(*id).map_err(err),
        _ => Ok(o),
    }
}

fn resolved_dict(doc: &Document, o: &Object) -> AppResult<Dictionary> {
    resolve(doc, o)?.as_dict().cloned().map_err(err)
}

pub(crate) fn decode_text(o: &Object) -> Option<String> {
    let Object::String(b, _) = o else { return None };
    if b.len() >= 2 && b[0] == 0xFE && b[1] == 0xFF {
        let u: Vec<u16> = b[2..].chunks(2).map(|c| u16::from_be_bytes([c[0], *c.get(1).unwrap_or(&0)])).collect();
        Some(String::from_utf16_lossy(&u))
    } else {
        Some(b.iter().map(|&c| c as char).collect())
    }
}

fn text_string(s: &str) -> Object {
    let mut b = vec![0xFE, 0xFF];
    for u in s.encode_utf16() {
        b.extend_from_slice(&u.to_be_bytes());
    }
    Object::String(b, StringFormat::Hexadecimal)
}

#[derive(Clone)]
struct Field {
    id: ObjectId,
    widgets: Vec<ObjectId>,
    is_text: bool,
    multiline: bool,
}

fn walk(doc: &Document, id: ObjectId, prefix: &str, ft: Option<Vec<u8>>, ff: i64, out: &mut BTreeMap<String, Field>) -> AppResult<()> {
    let d = doc.get_dictionary(id).map_err(err)?;
    let name = match d.get(b"T").ok().and_then(decode_text) {
        Some(p) if prefix.is_empty() => p,
        Some(p) => format!("{prefix}.{p}"),
        None => prefix.to_string(),
    };
    let ft = d.get(b"FT").ok().and_then(|o| o.as_name().ok().map(|n| n.to_vec())).or(ft);
    let ff = d.get(b"Ff").ok().and_then(|o| o.as_i64().ok()).unwrap_or(ff);
    let mut widgets = Vec::new();
    match d.get(b"Kids") {
        Ok(kids) => {
            for k in resolve(doc, kids)?.as_array().map_err(err)? {
                let kid = k.as_reference().map_err(err)?;
                if doc.get_dictionary(kid).map_err(err)?.has(b"T") {
                    walk(doc, kid, &name, ft.clone(), ff, out)?;
                } else {
                    widgets.push(kid);
                }
            }
        }
        Err(_) => widgets.push(id),
    }
    if !widgets.is_empty() {
        out.insert(name, Field { id, widgets, is_text: ft.as_deref() == Some(b"Tx"), multiline: ff & 4096 != 0 });
    }
    Ok(())
}

fn fields(doc: &Document) -> AppResult<BTreeMap<String, Field>> {
    let acro = resolved_dict(doc, doc.catalog().map_err(err)?.get(b"AcroForm").map_err(err)?)?;
    let mut out = BTreeMap::new();
    for f in resolve(doc, acro.get(b"Fields").map_err(err)?)?.as_array().map_err(err)? {
        walk(doc, f.as_reference().map_err(err)?, "", None, 0, &mut out)?;
    }
    Ok(out)
}

/// Remainder of `text` after the first `consumed` non-whitespace characters (wrap() drops only spaces).
fn rest_after(text: &str, consumed: usize) -> String {
    let mut n = 0;
    for (i, c) in text.char_indices() {
        if n == consumed {
            return text[i..].trim_start().to_string();
        }
        if !c.is_whitespace() {
            n += 1;
        }
    }
    String::new()
}

fn visible(lines: &[String]) -> usize {
    lines.iter().map(|l| l.chars().filter(|c| !c.is_whitespace()).count()).sum()
}

/// Choose a font size and lines for a field; returns (size, lines, overflow).
fn layout(fm: &FontMetrics, text: &str, w: f32, h: f32, multiline: bool) -> (f32, Vec<String>, Option<String>) {
    let inner = w - 2.0 * PAD;
    if !multiline {
        let one = text.replace(['\r', '\n'], " ");
        let mut size = MAX_SIZE.min(h - 2.0 * PAD).max(MIN_SIZE);
        while fm.width(&one, size) > inner && size > MIN_SIZE {
            size -= 0.5;
        }
        if fm.width(&one, size) <= inner {
            return (size, vec![one], None);
        }
        let first = wrap(fm, &one, inner, size).into_iter().next().unwrap_or_default();
        let rest = rest_after(&one, visible(std::slice::from_ref(&first)));
        return (size, vec![first], Some(rest).filter(|r| !r.is_empty()));
    }
    let mut size = MAX_SIZE;
    loop {
        let lines = wrap(fm, text, inner, size);
        let cap = ((h - 2.0 * PAD) / (size * LINE)).floor().max(1.0) as usize;
        if lines.len() <= cap {
            return (size, lines, None);
        }
        if size <= MIN_MULTILINE {
            let kept: Vec<String> = lines.into_iter().take(cap).collect();
            let rest = rest_after(text, visible(&kept));
            return (size, kept, Some(rest).filter(|r| !r.is_empty()));
        }
        size -= 0.5;
    }
}

fn show(fm: &FontMetrics, used: &mut BTreeMap<u16, char>, s: &str) -> String {
    let mut hex = String::with_capacity(s.len() * 4);
    for c in s.chars() {
        let g = fm.gid(c);
        used.entry(g).or_insert(c);
        hex.push_str(&format!("{g:04X}"));
    }
    format!("<{hex}> Tj")
}

fn appearance(fm: &FontMetrics, used: &mut BTreeMap<u16, char>, w: f32, h: f32, size: f32, lines: &[String], multiline: bool) -> String {
    let mut s = format!(
        "/Tx BMC\nq\n{:.2} {:.2} {:.2} {:.2} re W n\nBT\n/{FONT_RES} {size:.2} Tf\n0 g\n",
        PAD - 1.0,
        PAD - 1.0,
        w - 2.0 * PAD + 2.0,
        h - 2.0 * PAD + 2.0
    );
    let mut y = if multiline { h - PAD - size * 0.93 } else { (h - size * 0.73) / 2.0 };
    for l in lines {
        s.push_str(&format!("1 0 0 1 {PAD:.2} {y:.2} Tm {}\n", show(fm, used, l)));
        y -= size * LINE;
    }
    s.push_str("ET\nQ\nEMC\n");
    s
}

fn rect(doc: &Document, widget: ObjectId) -> AppResult<(f32, f32)> {
    let r = doc.get_dictionary(widget).map_err(err)?.get(b"Rect").map_err(err)?;
    let v: Vec<f32> = resolve(doc, r)?.as_array().map_err(err)?.iter().map(|o| o.as_float().map_err(err)).collect::<AppResult<_>>()?;
    Ok(((v[2] - v[0]).abs(), (v[3] - v[1]).abs()))
}

fn font_resources(doc: &Document, res: Option<&Object>, font_id: ObjectId) -> AppResult<Dictionary> {
    let mut res = match res {
        Some(o) => resolved_dict(doc, o)?,
        None => Dictionary::new(),
    };
    let mut fonts = match res.get(b"Font") {
        Ok(o) => resolved_dict(doc, o)?,
        Err(_) => Dictionary::new(),
    };
    fonts.set(FONT_RES, Object::Reference(font_id));
    res.set("Font", Object::Dictionary(fonts));
    Ok(res)
}

fn page_resources(doc: &Document, page: ObjectId) -> Option<Object> {
    let mut id = page;
    loop {
        let d = doc.get_dictionary(id).ok()?;
        if let Ok(r) = d.get(b"Resources") {
            return Some(r.clone());
        }
        id = d.get(b"Parent").ok()?.as_reference().ok()?;
    }
}

fn last_partial(name: &str) -> &str {
    name.rsplit('.').next().unwrap_or(name)
}

/// One output PDF built from copies of an official form's pages.
pub struct FormBuilder {
    doc: Document,
    fm: FontMetrics,
    font_id: ObjectId,
    used: BTreeMap<u16, char>,
    pages_root: ObjectId,
    template_pages: Vec<ObjectId>,
    template_fields: BTreeMap<String, Field>,
    widget_owner: HashMap<ObjectId, String>,
    /// Output pages and the footer each one carries.
    out_pages: Vec<(ObjectId, String)>,
    fields: BTreeMap<String, Field>,
    top_fields: Vec<ObjectId>,
}

impl FormBuilder {
    pub fn new(template: &[u8]) -> AppResult<FormBuilder> {
        let doc = Document::load_mem(template).map_err(err)?;
        let template_fields = fields(&doc)?;
        let mut widget_owner = HashMap::new();
        for (name, f) in &template_fields {
            for w in &f.widgets {
                widget_owner.insert(*w, name.clone());
            }
        }
        let template_pages: Vec<ObjectId> = doc.get_pages().into_values().collect();
        let pages_root = doc.catalog().map_err(err)?.get(b"Pages").and_then(Object::as_reference).map_err(err)?;
        let mut doc = doc;
        let font_id = doc.new_object_id();
        Ok(FormBuilder {
            doc,
            fm: FontMetrics::load()?,
            font_id,
            used: BTreeMap::new(),
            pages_root,
            template_pages,
            template_fields,
            widget_owner,
            out_pages: Vec::new(),
            fields: BTreeMap::new(),
            top_fields: Vec::new(),
        })
    }

    pub fn page_count(&self) -> usize {
        self.out_pages.len()
    }

    /// Append a copy of template page `n` (1-based). Its fields are reachable as "<name><suffix>";
    /// suffixes must be unique per copy of the same page.
    pub fn add_page(&mut self, n: usize, suffix: &str, footer: &str) -> AppResult<()> {
        let src = *self.template_pages.get(n - 1).ok_or_else(|| AppError::internal(format!("ICS template has no page {n}")))?;
        let page_id = self.doc.new_object_id();
        let mut page = self.doc.get_dictionary(src).map_err(err)?.clone();
        let annots: Vec<Object> = match page.get(b"Annots") {
            Ok(o) => resolve(&self.doc, o)?.as_array().map_err(err)?.clone(),
            Err(_) => vec![],
        };
        let mut new_annots = Vec::new();
        let mut parents: HashMap<String, ObjectId> = HashMap::new();
        for a in annots {
            let Ok(w) = a.as_reference() else {
                new_annots.push(a);
                continue;
            };
            let Some(name) = self.widget_owner.get(&w).cloned() else {
                new_annots.push(Object::Reference(w));
                continue;
            };
            let f = self.template_fields[&name].clone();
            let key = format!("{name}{suffix}");
            let mut wd = self.doc.get_dictionary(w).map_err(err)?.clone();
            wd.set("P", Object::Reference(page_id));
            wd.remove(b"AP");
            let new_w;
            if f.id == w {
                // Field and widget are one dictionary.
                if wd.has(b"Parent") {
                    return Err(AppError::internal(format!("ICS template field \"{name}\" is nested; not supported")));
                }
                wd.set("T", text_string(&format!("{}{suffix}", last_partial(&name))));
                wd.remove(b"V");
                new_w = self.doc.add_object(wd);
                self.top_fields.push(new_w);
                self.fields.insert(key, Field { id: new_w, widgets: vec![new_w], ..f });
            } else {
                let parent = match parents.get(&key).copied().or_else(|| self.fields.get(&key).map(|x| x.id)) {
                    Some(p) => p,
                    None => {
                        let mut pd = self.doc.get_dictionary(f.id).map_err(err)?.clone();
                        if pd.has(b"Parent") {
                            return Err(AppError::internal(format!("ICS template field \"{name}\" is nested; not supported")));
                        }
                        pd.set("T", text_string(&format!("{}{suffix}", last_partial(&name))));
                        pd.set("Kids", Object::Array(vec![]));
                        pd.remove(b"V");
                        let p = self.doc.add_object(pd);
                        self.top_fields.push(p);
                        self.fields.insert(key.clone(), Field { id: p, widgets: vec![], ..f.clone() });
                        p
                    }
                };
                parents.insert(key.clone(), parent);
                wd.set("Parent", Object::Reference(parent));
                new_w = self.doc.add_object(wd);
                if let Ok(Object::Array(kids)) = self.doc.get_dictionary_mut(parent).map_err(err)?.get_mut(b"Kids") {
                    kids.push(Object::Reference(new_w));
                }
                self.fields.get_mut(&key).expect("parent registered").widgets.push(new_w);
            }
            new_annots.push(Object::Reference(new_w));
        }
        page.set("Annots", Object::Array(new_annots));
        page.set("Parent", Object::Reference(self.pages_root));
        let res = font_resources(&self.doc, page_resources(&self.doc, src).as_ref(), self.font_id)?;
        page.set("Resources", Object::Dictionary(res));
        self.doc.objects.insert(page_id, Object::Dictionary(page));
        self.out_pages.push((page_id, footer.to_string()));
        Ok(())
    }

    /// Fill a text field on a page added earlier (name includes its copy suffix). Returns the part
    /// of `text` that did not fit, if any.
    pub fn set(&mut self, name: &str, text: &str) -> AppResult<Option<String>> {
        let f = self.fields.get(name).cloned().ok_or_else(|| AppError::internal(format!("ICS form has no field named \"{name}\"")))?;
        if !f.is_text {
            return Err(AppError::internal(format!("ICS form field \"{name}\" is not a text field")));
        }
        if text.is_empty() {
            return Ok(None);
        }
        let mut shown = text.to_string();
        let mut overflow = None;
        let mut da_size = MAX_SIZE;
        for (i, w) in f.widgets.iter().enumerate() {
            let (wd, ht) = rect(&self.doc, *w)?;
            let (size, lines, rest) = layout(&self.fm, text, wd, ht, f.multiline);
            if i == 0 {
                da_size = size;
                if let Some(r) = rest {
                    shown = text[..text.len() - r.len()].trim_end().to_string();
                    overflow = Some(r);
                }
            }
            let ap = appearance(&self.fm, &mut self.used, wd, ht, size, &lines, f.multiline);
            let mut xd = Dictionary::new();
            xd.set("Type", Object::Name(b"XObject".to_vec()));
            xd.set("Subtype", Object::Name(b"Form".to_vec()));
            xd.set("BBox", Object::Array(vec![0.into(), 0.into(), Object::Real(wd), Object::Real(ht)]));
            xd.set("Resources", Object::Dictionary(font_resources(&self.doc, None, self.font_id)?));
            let xo = self.doc.add_object(Stream::new(xd, ap.into_bytes()));
            let wdict = self.doc.get_dictionary_mut(*w).map_err(err)?;
            let mut apd = Dictionary::new();
            apd.set("N", Object::Reference(xo));
            wdict.set("AP", Object::Dictionary(apd));
            if wdict.has(b"DA") {
                wdict.set("DA", Object::string_literal(format!("/{FONT_RES} {size:.1} Tf 0 g")));
            }
        }
        let fdict = self.doc.get_dictionary_mut(f.id).map_err(err)?;
        fdict.set("V", text_string(&shown));
        fdict.set("DA", Object::string_literal(format!("/{FONT_RES} {da_size:.1} Tf 0 g")));
        Ok(overflow)
    }

    /// Append one or more plain continuation pages: a title, then `body` wrapped to the page.
    pub fn add_text_pages(&mut self, title: &str, body: &str, footer: &str) -> AppResult<()> {
        let (w, h) = LETTER;
        let (left, top, bottom, size, lead) = (36.0, h - 48.0, 52.0, 9.5, 12.5);
        let lines = wrap(&self.fm, body, w - 2.0 * left, size);
        let mut i = 0;
        let mut first = true;
        while first || i < lines.len() {
            let heading = if first { title.to_string() } else { format!("{title} (continued)") };
            let mut ops = format!("BT\n/{FONT_RES} 12 Tf\n0 g\n1 0 0 1 {left} {top} Tm {}\n", show(&self.fm, &mut self.used, &heading));
            let mut y = top - 24.0;
            while i < lines.len() && y >= bottom {
                ops.push_str(&format!("/{FONT_RES} {size} Tf\n1 0 0 1 {left} {y:.2} Tm {}\n", show(&self.fm, &mut self.used, &lines[i])));
                y -= lead;
                i += 1;
            }
            ops.push_str("ET\n");
            let content = self.doc.add_object(Stream::new(Dictionary::new(), ops.into_bytes()));
            let mut page = Dictionary::new();
            page.set("Type", Object::Name(b"Page".to_vec()));
            page.set("Parent", Object::Reference(self.pages_root));
            page.set("MediaBox", Object::Array(vec![0.into(), 0.into(), Object::Real(w), Object::Real(h)]));
            page.set("Resources", Object::Dictionary(font_resources(&self.doc, None, self.font_id)?));
            page.set("Contents", Object::Array(vec![Object::Reference(content)]));
            let id = self.doc.add_object(page);
            self.out_pages.push((id, footer.to_string()));
            first = false;
        }
        Ok(())
    }

    /// Assemble the output: only the added pages, their fields, page footers ("… Page n of N") and
    /// the embedded font. Template pages and fields that were not copied are dropped.
    pub fn finish(mut self, title: &str) -> AppResult<Vec<u8>> {
        if self.out_pages.is_empty() {
            return Err(AppError::internal("ICS form has no pages"));
        }
        let total = self.out_pages.len();
        for (n, (page, footer)) in self.out_pages.clone().into_iter().enumerate() {
            let text = format!("{footer}   Page {} of {total}", n + 1);
            let ops = format!("q\nBT\n/{FONT_RES} 6.5 Tf\n0.35 g\n1 0 0 1 36 22 Tm {}\nET\nQ\n", show(&self.fm, &mut self.used, &text));
            let stream_id = self.doc.add_object(Stream::new(Dictionary::new(), ops.into_bytes()));
            let pd = self.doc.get_dictionary_mut(page).map_err(err)?;
            let mut contents = match pd.get(b"Contents") {
                Ok(Object::Array(a)) => a.clone(),
                Ok(o) => vec![o.clone()],
                Err(_) => vec![],
            };
            contents.push(Object::Reference(stream_id));
            pd.set("Contents", Object::Array(contents));
        }
        let kids: Vec<Object> = self.out_pages.iter().map(|(id, _)| Object::Reference(*id)).collect();
        {
            let root = self.doc.get_dictionary_mut(self.pages_root).map_err(err)?;
            root.set("Kids", Object::Array(kids));
            root.set("Count", Object::Integer(total as i64));
        }

        let cat = self.doc.trailer.get(b"Root").and_then(Object::as_reference).map_err(err)?;
        let acro_obj = self.doc.get_dictionary(cat).map_err(err)?.get(b"AcroForm").map_err(err)?.clone();
        let mut acro = resolved_dict(&self.doc, &acro_obj)?;
        let dr = font_resources(&self.doc, acro.get(b"DR").ok(), self.font_id)?;
        acro.set("DR", Object::Dictionary(dr));
        acro.set("Fields", Object::Array(self.top_fields.iter().map(|id| Object::Reference(*id)).collect()));
        match acro_obj {
            Object::Reference(id) => *self.doc.get_object_mut(id).map_err(err)? = Object::Dictionary(acro),
            _ => self.doc.get_dictionary_mut(cat).map_err(err)?.set("AcroForm", Object::Dictionary(acro)),
        }
        let mut info = Dictionary::new();
        info.set("Title", text_string(title));
        info.set("Producer", text_string(&format!("EMCOMM LOG {}", crate::logbook::APP_VERSION)));
        let info_id = self.doc.add_object(info);
        self.doc.trailer.set("Info", Object::Reference(info_id));

        insert_font(&mut self.doc, &self.fm, self.font_id, &self.used)?;
        self.doc.prune_objects();
        self.doc.compress();
        let mut pdf = Vec::new();
        self.doc.save_to(&mut pdf).map_err(|e| AppError::internal(format!("Writing ICS form: {e}")))?;
        Ok(pdf)
    }
}

/// A PDF drawn from scratch (no official template), sharing the form filler's embedded font.
/// Used for the ICS 309, which FEMA does not publish as a fillable form.
pub struct DrawnDoc {
    doc: Document,
    fm: FontMetrics,
    font_id: ObjectId,
    pages_id: ObjectId,
    used: BTreeMap<u16, char>,
    pages: Vec<ObjectId>,
}

impl DrawnDoc {
    pub fn new() -> AppResult<DrawnDoc> {
        let mut doc = Document::with_version("1.7");
        let pages_id = doc.new_object_id();
        let font_id = doc.new_object_id();
        Ok(DrawnDoc { doc, fm: FontMetrics::load()?, font_id, pages_id, used: BTreeMap::new(), pages: Vec::new() })
    }

    pub fn width(&self, s: &str, size: f32) -> f32 {
        self.fm.width(s, size)
    }

    pub fn wrap(&self, s: &str, width: f32, size: f32) -> Vec<String> {
        wrap(&self.fm, s, width, size)
    }

    /// Content-stream operators that draw `s` with its baseline at (x, y).
    pub fn text(&mut self, x: f32, y: f32, size: f32, s: &str) -> String {
        format!("BT /{FONT_RES} {size:.2} Tf 1 0 0 1 {x:.2} {y:.2} Tm {} ET\n", show(&self.fm, &mut self.used, s))
    }

    /// Text that is cut (never wrapped) to fit `width`, with an ellipsis when cut. For single-line
    /// cells whose full value appears elsewhere.
    pub fn text_fit(&mut self, x: f32, y: f32, size: f32, width: f32, s: &str) -> String {
        if self.fm.width(s, size) <= width {
            return self.text(x, y, size, s);
        }
        let mut t = String::new();
        for c in s.chars() {
            if self.fm.width(&format!("{t}{c}\u{2026}"), size) > width {
                break;
            }
            t.push(c);
        }
        self.text(x, y, size, &format!("{t}\u{2026}"))
    }

    pub fn add_page(&mut self, ops: String) {
        let content = self.doc.add_object(Stream::new(Dictionary::new(), ops.into_bytes()));
        let mut fonts = Dictionary::new();
        fonts.set(FONT_RES, Object::Reference(self.font_id));
        let mut res = Dictionary::new();
        res.set("Font", Object::Dictionary(fonts));
        let mut page = Dictionary::new();
        page.set("Type", Object::Name(b"Page".to_vec()));
        page.set("Parent", Object::Reference(self.pages_id));
        page.set("MediaBox", Object::Array(vec![0.into(), 0.into(), Object::Real(LETTER.0), Object::Real(LETTER.1)]));
        page.set("Resources", Object::Dictionary(res));
        page.set("Contents", Object::Reference(content));
        let id = self.doc.add_object(page);
        self.pages.push(id);
    }

    pub fn finish(mut self, title: &str) -> AppResult<Vec<u8>> {
        if self.pages.is_empty() {
            return Err(AppError::internal("Drawn form has no pages"));
        }
        let mut pages = Dictionary::new();
        pages.set("Type", Object::Name(b"Pages".to_vec()));
        pages.set("Kids", Object::Array(self.pages.iter().map(|id| Object::Reference(*id)).collect()));
        pages.set("Count", Object::Integer(self.pages.len() as i64));
        self.doc.objects.insert(self.pages_id, Object::Dictionary(pages));
        let mut cat = Dictionary::new();
        cat.set("Type", Object::Name(b"Catalog".to_vec()));
        cat.set("Pages", Object::Reference(self.pages_id));
        let cat_id = self.doc.add_object(cat);
        self.doc.trailer.set("Root", Object::Reference(cat_id));
        let mut info = Dictionary::new();
        info.set("Title", text_string(title));
        info.set("Producer", text_string(&format!("EMCOMM LOG {}", crate::logbook::APP_VERSION)));
        let info_id = self.doc.add_object(info);
        self.doc.trailer.set("Info", Object::Reference(info_id));
        insert_font(&mut self.doc, &self.fm, self.font_id, &self.used)?;
        self.doc.compress();
        let mut pdf = Vec::new();
        self.doc.save_to(&mut pdf).map_err(|e| AppError::internal(format!("Writing form: {e}")))?;
        Ok(pdf)
    }
}

fn insert_font(doc: &mut Document, fm: &FontMetrics, font_id: ObjectId, used: &BTreeMap<u16, char>) -> AppResult<()> {
    let scale = 1000.0 / fm.upem;
    let mut widths = Vec::new();
    for g in used.keys() {
        widths.push(Object::Integer(*g as i64));
        widths.push(Object::Array(vec![Object::Integer((fm.advance(*g) as f32 * scale).round() as i64)]));
    }
    let mut fd = Dictionary::new();
    fd.set("Length1", Object::Integer(FONT_DATA.len() as i64));
    let mut file = Stream::new(fd, FONT_DATA.to_vec());
    let _ = file.compress();
    let file_id = doc.add_object(file);
    let bbox = fm.face.global_bounding_box();
    let n = |v: i16| Object::Integer((v as f32 * scale).round() as i64);
    let mut desc = Dictionary::new();
    desc.set("Type", Object::Name(b"FontDescriptor".to_vec()));
    desc.set("FontName", Object::Name(b"DejaVuSans".to_vec()));
    desc.set("Flags", Object::Integer(32));
    desc.set("FontBBox", Object::Array(vec![n(bbox.x_min), n(bbox.y_min), n(bbox.x_max), n(bbox.y_max)]));
    desc.set("ItalicAngle", Object::Integer(0));
    desc.set("Ascent", n(fm.face.ascender()));
    desc.set("Descent", n(fm.face.descender()));
    desc.set("CapHeight", n(fm.face.capital_height().unwrap_or(fm.face.ascender())));
    desc.set("StemV", Object::Integer(80));
    desc.set("FontFile2", Object::Reference(file_id));
    let desc_id = doc.add_object(desc);

    let mut sys = Dictionary::new();
    sys.set("Registry", Object::string_literal("Adobe"));
    sys.set("Ordering", Object::string_literal("Identity"));
    sys.set("Supplement", Object::Integer(0));
    let mut cid = Dictionary::new();
    cid.set("Type", Object::Name(b"Font".to_vec()));
    cid.set("Subtype", Object::Name(b"CIDFontType2".to_vec()));
    cid.set("BaseFont", Object::Name(b"DejaVuSans".to_vec()));
    cid.set("CIDSystemInfo", Object::Dictionary(sys));
    cid.set("FontDescriptor", Object::Reference(desc_id));
    cid.set("CIDToGIDMap", Object::Name(b"Identity".to_vec()));
    cid.set("DW", Object::Integer(600));
    cid.set("W", Object::Array(widths));
    let cid_id = doc.add_object(cid);

    let mut cmap = String::from(
        "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n/CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n",
    );
    let pairs: Vec<(u16, char)> = used.iter().map(|(g, c)| (*g, *c)).collect();
    for chunk in pairs.chunks(100) {
        cmap.push_str(&format!("{} beginbfchar\n", chunk.len()));
        for (g, c) in chunk {
            let mut buf = [0u16; 2];
            let u: String = c.encode_utf16(&mut buf).iter().map(|x| format!("{x:04X}")).collect();
            cmap.push_str(&format!("<{g:04X}> <{u}>\n"));
        }
        cmap.push_str("endbfchar\n");
    }
    cmap.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
    let cmap_id = doc.add_object(Stream::new(Dictionary::new(), cmap.into_bytes()));

    let mut font = Dictionary::new();
    font.set("Type", Object::Name(b"Font".to_vec()));
    font.set("Subtype", Object::Name(b"Type0".to_vec()));
    font.set("BaseFont", Object::Name(b"DejaVuSans".to_vec()));
    font.set("Encoding", Object::Name(b"Identity-H".to_vec()));
    font.set("DescendantFonts", Object::Array(vec![Object::Reference(cid_id)]));
    font.set("ToUnicode", Object::Reference(cmap_id));
    doc.objects.insert(font_id, Object::Dictionary(font));
    Ok(())
}

/// Field name -> value for every filled text field (for tests and verification).
pub fn read_values(pdf: &[u8]) -> AppResult<BTreeMap<String, String>> {
    let doc = Document::load_mem(pdf).map_err(err)?;
    let mut out = BTreeMap::new();
    for (name, f) in fields(&doc)? {
        if let Some(v) = doc.get_dictionary(f.id).map_err(err)?.get(b"V").ok().and_then(decode_text) {
            out.insert(name, v);
        }
    }
    Ok(out)
}

pub fn page_count(pdf: &[u8]) -> AppResult<usize> {
    Ok(Document::load_mem(pdf).map_err(err)?.get_pages().len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_hold_independent_values_and_share_one_font() {
        let mut b = FormBuilder::new(ICS_213).unwrap();
        b.add_page(1, "", "f1").unwrap();
        b.add_page(1, " (2)", "f2").unwrap();
        assert!(b.set("4 Subject", "First ✓").unwrap().is_none());
        assert!(b.set("4 Subject (2)", "José second").unwrap().is_none());
        let pdf = b.finish("t").unwrap();
        assert_eq!(page_count(&pdf).unwrap(), 2, "instruction page dropped, two copies");
        let v = read_values(&pdf).unwrap();
        assert_eq!(v["4 Subject"], "First ✓");
        assert_eq!(v["4 Subject (2)"], "José second");
        assert!(!v.contains_key("Signature_19"));
        let doc = Document::load_mem(&pdf).unwrap();
        let fonts = doc.objects.values().filter(|o| matches!(o.as_dict(), Ok(d) if d.get(b"BaseFont").ok() == Some(&Object::Name(b"DejaVuSans".to_vec())) && d.get(b"Subtype").ok() == Some(&Object::Name(b"Type0".to_vec())))).count();
        assert_eq!(fonts, 1);
    }

    #[test]
    fn shared_field_widgets_follow_their_copy() {
        // "Date From" has a widget on 214 pages 1 and 2 in the template.
        let mut b = FormBuilder::new(ICS_214).unwrap();
        b.add_page(1, "", "a").unwrap();
        b.add_page(2, " (2)", "b").unwrap();
        b.set("Date From", "2026-11-03").unwrap();
        b.set("Date From (2)", "2026-11-04").unwrap();
        let v = read_values(&b.finish("t").unwrap()).unwrap();
        assert_eq!(v["Date From"], "2026-11-03");
        assert_eq!(v["Date From (2)"], "2026-11-04");
    }

    #[test]
    fn overflow_is_returned_not_dropped() {
        let long = (1..=1500).map(|i| format!("word{i}")).collect::<Vec<_>>().join(" ");
        let mut b = FormBuilder::new(ICS_213).unwrap();
        b.add_page(1, "", "x").unwrap();
        let rest = b.set("7 Message", &long).unwrap().expect("overflow");
        assert!(rest.starts_with("word") && long.ends_with(rest.as_str()));
        let v = read_values(&b.finish("t").unwrap()).unwrap();
        let joined = format!("{} {rest}", v["7 Message"]);
        assert_eq!(joined.split_whitespace().count(), 1500, "every word is either shown or overflowed");
    }

    #[test]
    fn text_pages_flow_over_several_pages() {
        let mut b = FormBuilder::new(ICS_213).unwrap();
        b.add_page(1, "", "x").unwrap();
        let body = (1..=200).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
        b.add_text_pages("Continuation", &body, "x").unwrap();
        let pdf = b.finish("t").unwrap();
        assert!(page_count(&pdf).unwrap() >= 4);
    }

    #[test]
    fn unknown_field_is_an_error() {
        let mut b = FormBuilder::new(ICS_213).unwrap();
        b.add_page(1, "", "x").unwrap();
        assert!(b.set("No Such Field", "x").is_err());
        assert!(b.set("Signature_19", "x").is_err(), "signatures are not text fields");
    }
}
