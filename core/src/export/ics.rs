//! ICS forms: fill FEMA's official AcroForm PDFs (bundled in `assets/ics`, see docs/ICS_FIELD_MAP.md).
//! The forms default to Helvetica with no `NeedAppearances`, so every filled field gets its own
//! appearance stream drawn with the embedded DejaVu Sans (Unicode), and its `/DA` names that font so
//! the result stays editable. Signature fields are never touched. Text that does not fit a field is
//! never dropped: it is returned as overflow for the caller to continue elsewhere.

use super::pdf::{FONT_DATA, FontMetrics, wrap};
use crate::error::{AppError, AppResult};
use lopdf::{Dictionary, Document, Object, ObjectId, Stream, StringFormat};
use std::collections::BTreeMap;

pub static ICS_213: &[u8] = include_bytes!("../../assets/ics/ICS-213-v3.pdf");
pub static ICS_214: &[u8] = include_bytes!("../../assets/ics/ICS-214-v3.1.pdf");

const FONT_RES: &str = "EmDejaVu";
const MAX_SIZE: f32 = 9.0;
const MIN_SIZE: f32 = 6.0;
const MIN_MULTILINE: f32 = 7.0;
const PAD: f32 = 2.0;
const LINE: f32 = 1.15;

pub struct FilledForm {
    pub pdf: Vec<u8>,
    /// Field name -> the part of its text that did not fit.
    pub overflow: BTreeMap<String, String>,
}

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

fn decode_text(o: &Object) -> Option<String> {
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

/// Fill `values` (field name -> text) into the form `template`, drop the 1-based `drop_pages`
/// (instruction pages) and stamp `footer` on every remaining page.
pub fn fill(template: &[u8], values: &BTreeMap<&str, String>, drop_pages: &[u32], footer: &str) -> AppResult<FilledForm> {
    let fm = FontMetrics::load()?;
    let mut doc = Document::load_mem(template).map_err(err)?;
    let all = fields(&doc)?;
    let font_id = doc.new_object_id();
    let mut used: BTreeMap<u16, char> = BTreeMap::new();
    let mut overflow = BTreeMap::new();

    for (name, text) in values {
        let f = all.get(*name).ok_or_else(|| AppError::internal(format!("ICS form has no field named \"{name}\"")))?;
        if !f.is_text {
            return Err(AppError::internal(format!("ICS form field \"{name}\" is not a text field")));
        }
        let mut shown = text.clone();
        let mut da_size = MAX_SIZE;
        for (i, w) in f.widgets.iter().enumerate() {
            let (wd, ht) = rect(&doc, *w)?;
            let (size, lines, rest) = layout(&fm, text, wd, ht, f.multiline);
            if i == 0 {
                da_size = size;
                if let Some(r) = rest {
                    shown = text[..text.len() - r.len()].trim_end().to_string();
                    overflow.insert((*name).to_string(), r);
                }
            }
            let ap = appearance(&fm, &mut used, wd, ht, size, &lines, f.multiline);
            let mut xd = Dictionary::new();
            xd.set("Type", Object::Name(b"XObject".to_vec()));
            xd.set("Subtype", Object::Name(b"Form".to_vec()));
            xd.set("BBox", Object::Array(vec![0.into(), 0.into(), Object::Real(wd), Object::Real(ht)]));
            xd.set("Resources", Object::Dictionary(font_resources(&doc, None, font_id)?));
            let xo = doc.add_object(Stream::new(xd, ap.into_bytes()));
            let wdict = doc.get_object_mut(*w).map_err(err)?.as_dict_mut().map_err(err)?;
            let mut apd = Dictionary::new();
            apd.set("N", Object::Reference(xo));
            wdict.set("AP", Object::Dictionary(apd));
            if wdict.has(b"DA") {
                wdict.set("DA", Object::string_literal(format!("/{FONT_RES} {size:.1} Tf 0 g")));
            }
        }
        let fdict = doc.get_object_mut(f.id).map_err(err)?.as_dict_mut().map_err(err)?;
        fdict.set("V", text_string(&shown));
        fdict.set("DA", Object::string_literal(format!("/{FONT_RES} {da_size:.1} Tf 0 g")));
    }

    // AcroForm default resources must know the font so viewers can re-render edited fields.
    let root = doc.trailer.get(b"Root").and_then(Object::as_reference).map_err(err)?;
    let acro_obj = doc.get_dictionary(root).map_err(err)?.get(b"AcroForm").map_err(err)?.clone();
    let mut acro = resolved_dict(&doc, &acro_obj)?;
    let dr = font_resources(&doc, acro.get(b"DR").ok(), font_id)?;
    acro.set("DR", Object::Dictionary(dr));
    match acro_obj {
        Object::Reference(id) => *doc.get_object_mut(id).map_err(err)? = Object::Dictionary(acro),
        _ => doc.get_object_mut(root).map_err(err)?.as_dict_mut().map_err(err)?.set("AcroForm", Object::Dictionary(acro)),
    }

    if !drop_pages.is_empty() {
        doc.delete_pages(drop_pages);
    }
    let pages: Vec<ObjectId> = doc.get_pages().into_values().collect();
    for (n, page) in pages.iter().enumerate() {
        let text = format!("{footer}   Page {} of {}", n + 1, pages.len());
        let ops = format!("q\nBT\n/{FONT_RES} 6.5 Tf\n0.35 g\n1 0 0 1 36 22 Tm {}\nET\nQ\n", show(&fm, &mut used, &text));
        let stream_id = doc.add_object(Stream::new(Dictionary::new(), ops.into_bytes()));
        let res = font_resources(&doc, page_resources(&doc, *page).as_ref(), font_id)?;
        let pd = doc.get_object_mut(*page).map_err(err)?.as_dict_mut().map_err(err)?;
        let mut contents = match pd.get(b"Contents") {
            Ok(Object::Array(a)) => a.clone(),
            Ok(o) => vec![o.clone()],
            Err(_) => vec![],
        };
        contents.push(Object::Reference(stream_id));
        pd.set("Contents", Object::Array(contents));
        pd.set("Resources", Object::Dictionary(res));
    }

    insert_font(&mut doc, &fm, font_id, &used)?;
    doc.prune_objects();
    doc.compress();
    let mut pdf = Vec::new();
    doc.save_to(&mut pdf).map_err(|e| AppError::internal(format!("Writing ICS form: {e}")))?;
    Ok(FilledForm { pdf, overflow })
}

fn insert_font(doc: &mut Document, fm: &FontMetrics, font_id: ObjectId, used: &BTreeMap<u16, char>) -> AppResult<()> {
    let scale = 1000.0 / fm.upem;
    let mut widths = Vec::new();
    for g in used.keys() {
        widths.push(Object::Integer(*g as i64));
        widths.push(Object::Array(vec![Object::Integer((fm.advance(*g) as f32 * scale).round() as i64)]));
    }
    let mut file = Stream::new(
        {
            let mut d = Dictionary::new();
            d.set("Length1", Object::Integer(FONT_DATA.len() as i64));
            d
        },
        FONT_DATA.to_vec(),
    );
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_213() -> BTreeMap<&'static str, String> {
        BTreeMap::from([
            ("1 Incident Name Optional", "Lake County Storm ✓".to_string()),
            ("2 To Name and Position", "Jane Doe, Shelter Manager".to_string()),
            ("3 From Name and Position", "José Núñez, EOC Radio Operator".to_string()),
            ("4 Subject", "Shelter supply request".to_string()),
            ("5 Date", "2026-11-03".to_string()),
            ("6 Time", "08:08 -05:00".to_string()),
            ("7 Message", "Need 40 cots and 80 blankets at Pine Rd shelter by 18:00.\nContact on TAC 2.".to_string()),
            ("8 Approved by Name", "R. Smith".to_string()),
            ("PositionTitle_13", "Shelter Lead".to_string()),
        ])
    }

    #[test]
    fn fills_213_and_drops_instruction_page() {
        let out = fill(ICS_213, &sample_213(), &[2], "Prepared with EMCOMM LOG test").unwrap();
        assert!(out.overflow.is_empty());
        let doc = Document::load_mem(&out.pdf).unwrap();
        assert_eq!(doc.get_pages().len(), 1);
        let f = fields(&doc).unwrap();
        let v = |n: &str| doc.get_dictionary(f[n].id).unwrap().get(b"V").ok().and_then(decode_text);
        assert_eq!(v("3 From Name and Position").as_deref(), Some("José Núñez, EOC Radio Operator"));
        assert_eq!(v("1 Incident Name Optional").as_deref(), Some("Lake County Storm ✓"));
        assert_eq!(v("Signature_19"), None, "signature fields are never filled");
        for w in &f["7 Message"].widgets {
            assert!(doc.get_dictionary(*w).unwrap().has(b"AP"));
        }
    }

    #[test]
    fn overflow_is_returned_not_dropped() {
        let long = (1..=1500).map(|i| format!("word{i}")).collect::<Vec<_>>().join(" ");
        let vals = BTreeMap::from([("7 Message", long.clone()), ("8 Approved by Name", "Alexandria Montgomery-Vanderbilt III".into())]);
        let out = fill(ICS_213, &vals, &[2], "x").unwrap();
        let rest = &out.overflow["7 Message"];
        assert!(rest.starts_with("word") && long.ends_with(rest.as_str()));
        let doc = Document::load_mem(&out.pdf).unwrap();
        let f = fields(&doc).unwrap();
        let shown = decode_text(doc.get_dictionary(f["7 Message"].id).unwrap().get(b"V").unwrap()).unwrap();
        let joined = format!("{shown} {rest}");
        assert_eq!(joined.split_whitespace().count(), 1500, "every word is either shown or overflowed");
    }

    #[test]
    fn unknown_field_is_an_error() {
        let vals = BTreeMap::from([("No Such Field", "x".to_string())]);
        assert!(fill(ICS_213, &vals, &[], "x").is_err());
    }

    /// Writes a filled sample for viewing: EMCOMM_FORM_OUT=/path/out.pdf cargo test -p emcomm-core ics_sample -- --ignored
    #[test]
    #[ignore]
    fn ics_sample() {
        let path = std::env::var("EMCOMM_FORM_OUT").expect("EMCOMM_FORM_OUT");
        let mut vals = sample_213();
        vals.insert("9 Reply", "Approved. 40 cots and 80 blankets dispatched on truck 12, ETA 16:30.".into());
        vals.insert("10 Replied by Name", "M. Chen".into());
        vals.insert("PositionTitle_14", "Logistics Chief".into());
        vals.insert("DateTime_14", "2026-11-03 09:15 -05:00".into());
        let out = fill(ICS_213, &vals, &[2], "Prepared with EMCOMM LOG 1.0.0 from logbook \"Lake County Storm\" · entry #5 · message 0005")
            .unwrap();
        std::fs::write(path, out.pdf).unwrap();
    }
}
