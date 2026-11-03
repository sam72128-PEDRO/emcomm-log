//! PDF: print-ready landscape report generated from data. Self-contained writer with an embedded
//! DejaVu Sans CID font (Identity-H) plus a ToUnicode map so text is searchable and extractable.
//! Column headings repeat on every page; rows taller than a page continue on the next page;
//! "Page X of Y" uses a shared form XObject written after the last page (streaming friendly).

use super::{ReportContext, detail_lines};
use crate::entries::for_each_entry;
use crate::error::{AppError, AppResult};
use crate::models::{Entry, EntryQuery, EntryStatus, LogbookType};
use flate2::Compression;
use flate2::write::ZlibEncoder;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

static FONT_DATA: &[u8] = include_bytes!("../../assets/fonts/DejaVuSans.ttf");

const PAGE_W: f32 = 792.0;
const PAGE_H: f32 = 612.0;
const MARGIN: f32 = 36.0;
const FONT_SIZE: f32 = 8.0;
const LINE_H: f32 = 10.0;
const CELL_PAD: f32 = 3.0;

struct FontMetrics {
    face: ttf_parser::Face<'static>,
    upem: f32,
}

impl FontMetrics {
    fn load() -> AppResult<FontMetrics> {
        let face = ttf_parser::Face::parse(FONT_DATA, 0).map_err(|e| AppError::internal(format!("Embedded font invalid: {e}")))?;
        let upem = face.units_per_em() as f32;
        Ok(FontMetrics { face, upem })
    }
    fn gid(&self, c: char) -> u16 {
        self.face.glyph_index(c).map(|g| g.0).unwrap_or(0)
    }
    fn advance(&self, gid: u16) -> u16 {
        self.face.glyph_hor_advance(ttf_parser::GlyphId(gid)).unwrap_or(0)
    }
    fn width(&self, s: &str, size: f32) -> f32 {
        s.chars().map(|c| self.advance(self.gid(c)) as f32).sum::<f32>() * size / self.upem
    }
}

/// Wrap text to a width. Never drops characters: overlong words are broken by character.
fn wrap(fm: &FontMetrics, text: &str, width: f32, size: f32) -> Vec<String> {
    let mut lines = Vec::new();
    for para in text.replace('\t', "    ").split('\n') {
        let mut cur = String::new();
        for word in para.split(' ') {
            let candidate = if cur.is_empty() { word.to_string() } else { format!("{cur} {word}") };
            if fm.width(&candidate, size) <= width {
                cur = candidate;
                continue;
            }
            if !cur.is_empty() {
                lines.push(std::mem::take(&mut cur));
            }
            if fm.width(word, size) <= width {
                cur = word.to_string();
            } else {
                for ch in word.chars() {
                    let mut t = cur.clone();
                    t.push(ch);
                    if fm.width(&t, size) > width && !cur.is_empty() {
                        lines.push(std::mem::take(&mut cur));
                        cur.push(ch);
                    } else {
                        cur = t;
                    }
                }
            }
        }
        lines.push(cur);
    }
    lines
}

#[derive(Clone)]
struct Line {
    text: String,
    bold: bool,
}

struct PdfOut {
    w: BufWriter<File>,
    pos: u64,
    offsets: BTreeMap<u32, u64>,
    next_obj: u32,
    page_ids: Vec<u32>,
    used: BTreeMap<u16, char>,
}

const OBJ_CATALOG: u32 = 1;
const OBJ_PAGES: u32 = 2;
const OBJ_FONT: u32 = 3;
const OBJ_CIDFONT: u32 = 4;
const OBJ_DESC: u32 = 5;
const OBJ_FONTFILE: u32 = 6;
const OBJ_TOUNICODE: u32 = 7;
const OBJ_TOTAL: u32 = 8;
const OBJ_INFO: u32 = 9;

impl PdfOut {
    fn raw(&mut self, b: &[u8]) -> AppResult<()> {
        self.w.write_all(b)?;
        self.pos += b.len() as u64;
        Ok(())
    }
    fn begin(&mut self, id: u32) -> AppResult<()> {
        self.offsets.insert(id, self.pos);
        self.raw(format!("{id} 0 obj\n").as_bytes())
    }
    fn object(&mut self, id: u32, body: &str) -> AppResult<()> {
        self.begin(id)?;
        self.raw(body.as_bytes())?;
        self.raw(b"\nendobj\n")
    }
    fn stream(&mut self, id: u32, dict_extra: &str, data: &[u8]) -> AppResult<()> {
        let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
        enc.write_all(data)?;
        let comp = enc.finish()?;
        self.begin(id)?;
        self.raw(format!("<< /Length {} /Filter /FlateDecode {dict_extra} >>\nstream\n", comp.len()).as_bytes())?;
        self.raw(&comp)?;
        self.raw(b"\nendstream\nendobj\n")
    }
    fn alloc(&mut self) -> u32 {
        let id = self.next_obj;
        self.next_obj += 1;
        id
    }
}

struct Page {
    ops: String,
}

impl Page {
    #[allow(clippy::too_many_arguments)]
    fn text(&mut self, fm: &FontMetrics, used: &mut BTreeMap<u16, char>, x: f32, y: f32, size: f32, s: &str, bold: bool) {
        if s.is_empty() {
            return;
        }
        let mut hex = String::with_capacity(s.len() * 4);
        for c in s.chars() {
            let c = if c.is_control() { ' ' } else { c };
            let g = fm.gid(c);
            if g != 0 {
                used.entry(g).or_insert(c);
            }
            hex.push_str(&format!("{g:04X}"));
        }
        if bold {
            self.ops.push_str("2 Tr 0.25 w ");
        }
        self.ops.push_str(&format!("BT /F1 {size} Tf {x:.2} {y:.2} Td <{hex}> Tj ET\n"));
        if bold {
            self.ops.push_str("0 Tr ");
        }
    }
    fn rect_fill(&mut self, x: f32, y: f32, w: f32, h: f32, gray: f32) {
        self.ops.push_str(&format!("{gray} g {x:.2} {y:.2} {w:.2} {h:.2} re f 0 g\n"));
    }
    fn hline(&mut self, x1: f32, x2: f32, y: f32, width: f32) {
        self.ops.push_str(&format!("{width} w 0.6 G {x1:.2} {y:.2} m {x2:.2} {y:.2} l S 0 G\n"));
    }
}

const COLS: &[(&str, f32)] = &[
    ("#", 34.0),
    ("Event time", 100.0),
    ("Type / Dir / Pri", 82.0),
    ("From / To", 112.0),
    ("Summary, message and details", 344.0),
    ("Status", 48.0),
];

fn cell_lines(fm: &FontMetrics, e: &Entry, book: LogbookType, ctx: &ReportContext) -> Vec<Vec<Line>> {
    let w = |i: usize| COLS[i].1 - 2.0 * CELL_PAD;
    let plain = |i: usize, s: &str| wrap(fm, s, w(i), FONT_SIZE).into_iter().map(|t| Line { text: t, bold: false }).collect::<Vec<_>>();
    let mut msg: Vec<Line> = wrap(fm, &e.subject, w(4), FONT_SIZE).into_iter().map(|t| Line { text: t, bold: true }).collect();
    if !e.body.is_empty() {
        msg.extend(plain(4, &e.body));
    }
    let details: Vec<String> = detail_lines(e, book, ctx.zone)
        .into_iter()
        .filter(|(k, _)| !matches!(k.as_str(), "From" | "To" | "Recorded UTC" | "Entered Zone"))
        .map(|(k, v)| format!("{k}: {}", v.replace('\n', " / ")))
        .collect();
    if !details.is_empty() {
        msg.extend(plain(4, &details.join("  |  ")));
    }
    let mut status = e.status.as_str().to_uppercase();
    if e.status == EntryStatus::Void
        && let Some(r) = e.void_reason.as_deref().filter(|r| !r.is_empty())
    {
        status = format!("VOID: {r}");
    }
    vec![
        plain(0, &format!("{:04}", e.sequence_no)),
        plain(1, &format!("{}\nRecorded {}", ctx.zone.format(&e.event_at_utc), ctx.zone.format(&e.recorded_at_utc))),
        plain(2, &format!("{}\n{}\n{}", e.kind.as_str(), e.direction.as_str(), e.priority.as_str())),
        plain(
            3,
            &format!(
                "{}\n-> {}",
                if e.from_text.is_empty() { "-" } else { &e.from_text },
                if e.to_text.is_empty() { "-" } else { &e.to_text }
            ),
        ),
        msg,
        plain(5, &status),
    ]
}

struct Layout<'a> {
    fm: &'a FontMetrics,
    ctx: &'a ReportContext,
    out: PdfOut,
    page: Option<Page>,
    y: f32,
    page_no: u32,
    header_lines: Vec<String>,
}

impl<'a> Layout<'a> {
    fn table_top(&self) -> f32 {
        PAGE_H - MARGIN - (self.header_lines.len() as f32 + 1.0) * 11.0 - 4.0
    }
    fn bottom(&self) -> f32 {
        MARGIN + 18.0
    }

    fn new_page(&mut self) -> AppResult<()> {
        self.finish_page()?;
        self.page_no += 1;
        let mut p = Page { ops: String::new() };
        let mut y = PAGE_H - MARGIN - 10.0;
        let title = format!("EMCOMM LOG - {} ({})", self.ctx.logbook.name, self.ctx.logbook.logbook_type.label());
        p.text(self.fm, &mut self.out.used, MARGIN, y, 12.0, &title, true);
        y -= 13.0;
        for l in &self.header_lines {
            p.text(self.fm, &mut self.out.used, MARGIN, y, 7.5, l, false);
            y -= 11.0;
        }
        let top = self.table_top();
        p.rect_fill(MARGIN, top - 13.0, PAGE_W - 2.0 * MARGIN, 13.0, 0.88);
        let mut x = MARGIN;
        for (h, w) in COLS {
            p.text(self.fm, &mut self.out.used, x + CELL_PAD, top - 9.5, FONT_SIZE, h, true);
            x += w;
        }
        p.hline(MARGIN, PAGE_W - MARGIN, top - 13.0, 0.6);
        let footer = format!("Page {} of ", self.page_no);
        let fx = PAGE_W - MARGIN - 70.0;
        p.text(self.fm, &mut self.out.used, fx, MARGIN, 7.5, &footer, false);
        let tw = self.fm.width(&footer, 7.5);
        p.ops.push_str(&format!("q 1 0 0 1 {:.2} {:.2} cm /TP Do Q\n", fx + tw, MARGIN));
        p.text(
            self.fm,
            &mut self.out.used,
            MARGIN,
            MARGIN,
            7.5,
            &format!("Generated {} - times in {}", self.ctx.generated, self.ctx.zone.label()),
            false,
        );
        self.y = top - 13.0 - 2.0;
        self.page = Some(p);
        Ok(())
    }

    fn finish_page(&mut self) -> AppResult<()> {
        if let Some(p) = self.page.take() {
            let content = self.out.alloc();
            let page = self.out.alloc();
            self.out.stream(content, "", p.ops.as_bytes())?;
            self.out.object(
                page,
                &format!(
                    "<< /Type /Page /Parent {OBJ_PAGES} 0 R /MediaBox [0 0 {PAGE_W} {PAGE_H}] /Resources << /Font << /F1 {OBJ_FONT} 0 R >> /XObject << /TP {OBJ_TOTAL} 0 R >> >> /Contents {content} 0 R >>"
                ),
            )?;
            self.out.page_ids.push(page);
        }
        Ok(())
    }

    fn add_row(&mut self, cells: Vec<Vec<Line>>) -> AppResult<()> {
        let max_lines = cells.iter().map(|c| c.len()).max().unwrap_or(1).max(1);
        let mut start = 0usize;
        while start < max_lines {
            if self.page.is_none() {
                self.new_page()?;
            }
            let avail = ((self.y - self.bottom() - CELL_PAD) / LINE_H).floor() as usize;
            let remaining = max_lines - start;
            let full_page = ((self.table_top() - 15.0 - self.bottom() - CELL_PAD) / LINE_H).floor() as usize;
            if avail < remaining.clamp(1, 3) || (avail < remaining && remaining <= full_page && start == 0) {
                self.new_page()?;
                continue;
            }
            let take = remaining.min(avail.max(1));
            let top = self.y;
            let mut x = MARGIN;
            let page = self.page.as_mut().unwrap();
            for (i, col) in cells.iter().enumerate() {
                for (k, line) in col.iter().skip(start).take(take).enumerate() {
                    let ly = top - CELL_PAD - FONT_SIZE - k as f32 * LINE_H + 1.0;
                    page.text(self.fm, &mut self.out.used, x + CELL_PAD, ly, FONT_SIZE, &line.text, line.bold);
                }
                x += COLS[i].1;
            }
            self.y = top - take as f32 * LINE_H - 2.0 * CELL_PAD;
            page.hline(MARGIN, PAGE_W - MARGIN, self.y, 0.3);
            start += take;
            if start < max_lines {
                self.new_page()?;
            }
        }
        Ok(())
    }
}

fn utf16_hex(s: &str) -> String {
    let mut h = String::from("FEFF");
    for u in s.encode_utf16() {
        h.push_str(&format!("{u:04X}"));
    }
    h
}

pub fn write(conn: &Connection, q: &EntryQuery, ctx: &ReportContext, path: &Path) -> AppResult<i64> {
    let fm = FontMetrics::load()?;
    let mut out = PdfOut {
        w: BufWriter::new(File::create(path)?),
        pos: 0,
        offsets: BTreeMap::new(),
        next_obj: 10,
        page_ids: vec![],
        used: BTreeMap::new(),
    };
    out.raw(b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n")?;
    let usable = PAGE_W - 2.0 * MARGIN;
    let mut header_lines = Vec::new();
    let inc = &ctx.logbook.incident;
    if !inc.incident_name.is_empty() {
        header_lines.push(format!(
            "Incident: {}{}{}",
            inc.incident_name,
            if inc.incident_number.is_empty() { String::new() } else { format!(" #{}", inc.incident_number) },
            if inc.operational_period.is_empty() { String::new() } else { format!("   Operational period: {}", inc.operational_period) }
        ));
    }
    for l in wrap(&fm, &format!("Scope: {}   Records: {}", ctx.description, ctx.count), usable, 7.5) {
        header_lines.push(l);
    }
    let mut lay = Layout { fm: &fm, ctx, out, page: None, y: 0.0, page_no: 0, header_lines };
    let mut n = 0i64;
    for_each_entry(conn, q, |e| {
        let cells = cell_lines(&fm, e, ctx.logbook.logbook_type, ctx);
        lay.add_row(cells)?;
        n += 1;
        Ok(())
    })?;
    if lay.page.is_none() {
        lay.new_page()?;
    }
    let end_note = format!("End of report - {n} record(s).");
    let y = lay.y - 12.0;
    if y > lay.bottom() {
        let page = lay.page.as_mut().unwrap();
        page.text(&fm, &mut lay.out.used, MARGIN, y, FONT_SIZE, &end_note, true);
    } else {
        lay.new_page()?;
        let y = lay.y - 12.0;
        let page = lay.page.as_mut().unwrap();
        page.text(&fm, &mut lay.out.used, MARGIN, y, FONT_SIZE, &end_note, true);
    }
    lay.finish_page()?;
    let total = lay.page_no;
    let mut out = lay.out;

    // Total-pages form XObject, referenced from every page footer.
    let mut tp = Page { ops: String::new() };
    tp.text(&fm, &mut out.used, 0.0, 0.0, 7.5, &total.to_string(), false);
    let tp_ops = tp.ops.clone();
    out.stream(
        OBJ_TOTAL,
        &format!("/Type /XObject /Subtype /Form /BBox [0 -2 60 10] /Resources << /Font << /F1 {OBJ_FONT} 0 R >> >>"),
        tp_ops.as_bytes(),
    )?;

    let kids: Vec<String> = out.page_ids.iter().map(|id| format!("{id} 0 R")).collect();
    out.object(OBJ_PAGES, &format!("<< /Type /Pages /Kids [{}] /Count {} >>", kids.join(" "), out.page_ids.len()))?;
    out.object(OBJ_CATALOG, &format!("<< /Type /Catalog /Pages {OBJ_PAGES} 0 R >>"))?;

    let scale = 1000.0 / fm.upem;
    let mut wpart = String::new();
    for g in out.used.keys() {
        wpart.push_str(&format!("{g} [{}] ", (fm.advance(*g) as f32 * scale).round()));
    }
    let bbox = fm.face.global_bounding_box();
    let asc = (fm.face.ascender() as f32 * scale).round();
    let desc = (fm.face.descender() as f32 * scale).round();
    let cap = (fm.face.capital_height().unwrap_or(fm.face.ascender()) as f32 * scale).round();
    out.object(
        OBJ_FONT,
        &format!("<< /Type /Font /Subtype /Type0 /BaseFont /DejaVuSans /Encoding /Identity-H /DescendantFonts [{OBJ_CIDFONT} 0 R] /ToUnicode {OBJ_TOUNICODE} 0 R >>"),
    )?;
    out.object(
        OBJ_CIDFONT,
        &format!(
            "<< /Type /Font /Subtype /CIDFontType2 /BaseFont /DejaVuSans /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> /FontDescriptor {OBJ_DESC} 0 R /CIDToGIDMap /Identity /DW 600 /W [{wpart}] >>"
        ),
    )?;
    out.object(
        OBJ_DESC,
        &format!(
            "<< /Type /FontDescriptor /FontName /DejaVuSans /Flags 32 /FontBBox [{} {} {} {}] /ItalicAngle 0 /Ascent {asc} /Descent {desc} /CapHeight {cap} /StemV 80 /FontFile2 {OBJ_FONTFILE} 0 R >>",
            (bbox.x_min as f32 * scale).round(),
            (bbox.y_min as f32 * scale).round(),
            (bbox.x_max as f32 * scale).round(),
            (bbox.y_max as f32 * scale).round()
        ),
    )?;
    out.stream(OBJ_FONTFILE, &format!("/Length1 {}", FONT_DATA.len()), FONT_DATA)?;

    let mut cmap = String::from(
        "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n/CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n",
    );
    let pairs: Vec<(u16, char)> = out.used.iter().map(|(g, c)| (*g, *c)).collect();
    for chunk in pairs.chunks(100) {
        cmap.push_str(&format!("{} beginbfchar\n", chunk.len()));
        for (g, c) in chunk {
            let mut u = String::new();
            let mut buf = [0u16; 2];
            for x in c.encode_utf16(&mut buf) {
                u.push_str(&format!("{x:04X}"));
            }
            cmap.push_str(&format!("<{g:04X}> <{u}>\n"));
        }
        cmap.push_str("endbfchar\n");
    }
    cmap.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
    out.stream(OBJ_TOUNICODE, "", cmap.as_bytes())?;
    out.object(
        OBJ_INFO,
        &format!(
            "<< /Title <{}> /Producer (EMCOMM LOG {}) /CreationDate (D:{}Z) >>",
            utf16_hex(&format!("EMCOMM LOG - {}", ctx.logbook.name)),
            crate::logbook::APP_VERSION,
            chrono::Utc::now().format("%Y%m%d%H%M%S")
        ),
    )?;

    let xref_pos = out.pos;
    let max_id = out.next_obj - 1;
    let mut xref = format!("xref\n0 {}\n0000000000 65535 f \n", max_id + 1);
    for id in 1..=max_id {
        match out.offsets.get(&id) {
            Some(off) => xref.push_str(&format!("{off:010} 00000 n \n")),
            None => xref.push_str("0000000000 65535 f \n"),
        }
    }
    xref.push_str(&format!(
        "trailer\n<< /Size {} /Root {OBJ_CATALOG} 0 R /Info {OBJ_INFO} 0 R >>\nstartxref\n{xref_pos}\n%%EOF\n",
        max_id + 1
    ));
    out.raw(xref.as_bytes())?;
    out.w.flush()?;
    out.w.into_inner().map_err(|e| AppError::from(e.into_error()))?.sync_all()?;
    Ok(n)
}

use rusqlite::Connection;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_never_drops_characters() {
        let fm = FontMetrics::load().unwrap();
        let text = "short words then averyveryveryveryveryveryverylongwordthatmustbreak and\nnew line ünïcödé";
        let lines = wrap(&fm, text, 60.0, 8.0);
        let joined: String = lines.join("");
        let orig: String = text.chars().filter(|c| !c.is_whitespace()).collect();
        let got: String = joined.chars().filter(|c| !c.is_whitespace()).collect();
        assert_eq!(orig, got);
        assert!(lines.iter().all(|l| fm.width(l, 8.0) <= 60.0 + 0.01 || l.chars().count() == 1));
    }
}
