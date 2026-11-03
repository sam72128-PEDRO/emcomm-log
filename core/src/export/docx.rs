//! DOCX: an editable WordprocessingML report written directly (no Office dependency).
//! One section per entry with a heading kept with its content, labeled fields, full message
//! text, attachment names, and a footer with page numbers.

use super::{detail_lines, heading_line, ReportContext};
use crate::entries::for_each_entry;
use crate::error::{AppError, AppResult};
use crate::models::{EntryQuery, EntryStatus};
use rusqlite::Connection;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

pub fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\t' | '\n' | '\r' => out.push(c),
            c if (c as u32) < 0x20 || c == '\u{FFFE}' || c == '\u{FFFF}' => {}
            c => out.push(c),
        }
    }
    out
}

fn runs(text: &str, bold: bool) -> String {
    let rpr = if bold { "<w:rPr><w:b/></w:rPr>" } else { "" };
    let mut out = String::new();
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            out.push_str(&format!("<w:r>{rpr}<w:br/></w:r>"));
        }
        out.push_str(&format!("<w:r>{rpr}<w:t xml:space=\"preserve\">{}</w:t></w:r>", xml_escape(line)));
    }
    out
}

fn para(style: Option<&str>, keep_next: bool, content: &str) -> String {
    let mut ppr = String::new();
    if let Some(s) = style {
        ppr.push_str(&format!("<w:pStyle w:val=\"{s}\"/>"));
    }
    if keep_next {
        ppr.push_str("<w:keepNext/>");
    }
    format!("<w:p><w:pPr>{ppr}</w:pPr>{content}</w:p>")
}

fn labeled(label: &str, value: &str) -> String {
    para(Some("Field"), false, &format!("{}{}", runs(&format!("{label}: "), true), runs(value, false)))
}

const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="xml" ContentType="application/xml"/>
<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
<Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/>
<Override PartName="/word/footer1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml"/>
<Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/>
</Types>"#;

const ROOT_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/>
</Relationships>"#;

const DOC_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>
<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer" Target="footer1.xml"/>
</Relationships>"#;

const STYLES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="Calibri" w:hAnsi="Calibri" w:eastAsia="Calibri" w:cs="Calibri"/><w:sz w:val="20"/><w:szCs w:val="20"/></w:rPr></w:rPrDefault>
<w:pPrDefault><w:pPr><w:spacing w:after="60" w:line="259" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults>
<w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style>
<w:style w:type="paragraph" w:styleId="Title"><w:name w:val="Title"/><w:basedOn w:val="Normal"/><w:pPr><w:spacing w:after="120"/></w:pPr><w:rPr><w:b/><w:sz w:val="36"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="Heading2"><w:name w:val="heading 2"/><w:basedOn w:val="Normal"/><w:pPr><w:keepNext/><w:spacing w:before="240" w:after="60"/><w:pBdr><w:top w:val="single" w:sz="4" w:space="4" w:color="8899AA"/></w:pBdr><w:outlineLvl w:val="1"/></w:pPr><w:rPr><w:b/><w:sz w:val="22"/><w:color w:val="1F3B5A"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="Field"><w:name w:val="Field"/><w:basedOn w:val="Normal"/><w:pPr><w:spacing w:after="20"/></w:pPr></w:style>
<w:style w:type="paragraph" w:styleId="Meta"><w:name w:val="Meta"/><w:basedOn w:val="Normal"/><w:rPr><w:color w:val="444444"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="Footer"><w:name w:val="footer"/><w:basedOn w:val="Normal"/><w:pPr><w:jc w:val="center"/></w:pPr><w:rPr><w:sz w:val="16"/></w:rPr></w:style>
</w:styles>"#;

fn footer(title: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:ftr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p><w:pPr><w:pStyle w:val="Footer"/></w:pPr><w:r><w:t xml:space="preserve">{} - Page </w:t></w:r><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText xml:space="preserve"> PAGE </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>1</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r><w:r><w:t xml:space="preserve"> of </w:t></w:r><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText xml:space="preserve"> NUMPAGES </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>1</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p></w:ftr>"#,
        xml_escape(title)
    )
}

fn core_props(title: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"><dc:title>{}</dc:title><dc:creator>EMCOMM LOG</dc:creator><dcterms:created xsi:type="dcterms:W3CDTF">{}</dcterms:created></cp:coreProperties>"#,
        xml_escape(title),
        chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ")
    )
}

fn zip_err(e: zip::result::ZipError) -> AppError {
    match e {
        zip::result::ZipError::Io(io) => io.into(),
        other => AppError::internal(format!("DOCX archive error: {other}")),
    }
}

pub fn write(conn: &Connection, q: &EntryQuery, ctx: &ReportContext, path: &Path) -> AppResult<i64> {
    let lb = &ctx.logbook;
    let title = format!("EMCOMM LOG - {}", lb.name);
    let f = File::create(path)?;
    let mut z = zip::ZipWriter::new(BufWriter::new(f));
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (name, body) in [
        ("[Content_Types].xml", CONTENT_TYPES.to_string()),
        ("_rels/.rels", ROOT_RELS.to_string()),
        ("word/_rels/document.xml.rels", DOC_RELS.to_string()),
        ("word/styles.xml", STYLES.to_string()),
        ("word/footer1.xml", footer(&title)),
        ("docProps/core.xml", core_props(&title)),
    ] {
        z.start_file(name, opts).map_err(zip_err)?;
        z.write_all(body.as_bytes())?;
    }
    z.start_file("word/document.xml", opts.large_file(true)).map_err(zip_err)?;
    z.write_all(
        br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:body>"#,
    )?;
    let mut head = String::new();
    head.push_str(&para(Some("Title"), false, &runs(&title, false)));
    head.push_str(&para(Some("Meta"), false, &runs(&format!("{} logbook", lb.logbook_type.label()), false)));
    if !lb.incident.incident_name.is_empty() {
        let inc = format!(
            "Incident: {}{}{}",
            lb.incident.incident_name,
            if lb.incident.incident_number.is_empty() { String::new() } else { format!(" #{}", lb.incident.incident_number) },
            if lb.incident.operational_period.is_empty() { String::new() } else { format!(" - Operational period {}", lb.incident.operational_period) }
        );
        head.push_str(&para(Some("Meta"), false, &runs(&inc, false)));
    }
    head.push_str(&para(Some("Meta"), false, &runs(&format!("Generated: {}", ctx.generated), false)));
    head.push_str(&para(Some("Meta"), false, &runs(&format!("Times shown in: {}", ctx.zone.label()), false)));
    head.push_str(&para(Some("Meta"), false, &runs(&format!("Scope: {}", ctx.description), false)));
    head.push_str(&para(Some("Meta"), false, &runs(&format!("Records: {}", ctx.count), false)));
    z.write_all(head.as_bytes())?;
    let mut n = 0i64;
    for_each_entry(conn, q, |e| {
        let mut s = String::new();
        s.push_str(&para(Some("Heading2"), true, &runs(&heading_line(e, ctx.zone), false)));
        s.push_str(&labeled("Summary", &e.subject));
        if e.status == EntryStatus::Void {
            s.push_str(&labeled("VOIDED", e.void_reason.as_deref().filter(|r| !r.is_empty()).unwrap_or("(no reason given)")));
        }
        for (k, v) in detail_lines(e, lb.logbook_type, ctx.zone) {
            s.push_str(&labeled(&k, &v));
        }
        if !e.body.is_empty() {
            s.push_str(&para(Some("Field"), true, &runs("Message:", true)));
            for line in e.body.split('\n') {
                s.push_str(&para(None, false, &runs(line, false)));
            }
        }
        z.write_all(s.as_bytes())?;
        n += 1;
        Ok(())
    })?;
    let end = format!(
        "{}<w:sectPr><w:footerReference w:type=\"default\" r:id=\"rId2\"/><w:pgSz w:w=\"12240\" w:h=\"15840\"/><w:pgMar w:top=\"1080\" w:right=\"1080\" w:bottom=\"1080\" w:left=\"1080\" w:header=\"720\" w:footer=\"540\" w:gutter=\"0\"/></w:sectPr></w:body></w:document>",
        para(Some("Meta"), false, &runs(&format!("End of report - {n} record(s)."), false))
    );
    z.write_all(end.as_bytes())?;
    let w = z.finish().map_err(zip_err)?;
    w.into_inner().map_err(|e| AppError::from(e.into_error()))?.sync_all()?;
    Ok(n)
}
