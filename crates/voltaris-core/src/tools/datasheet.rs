//! `datasheet` toolset — fetch a part's PDF datasheet, extract its text page
//! by page, and return the sections a designer needs (pins, absolute maximum
//! ratings, operating conditions, application circuit, layout guidance,
//! package) plus page-cited matches for any query.

use crate::mcp::protocol::{CallToolResult, ToolContent};
use crate::tool;
use crate::tools::{invalid_arg, opt_str, ToolContext, ToolDef};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

const SECTIONS: &[(&str, &[&str])] = &[
    (
        "pin_description",
        &[
            "pin description",
            "pin functions",
            "pin function",
            "pin configuration",
            "pin assignment",
            "pin definitions",
            "pinout",
            "pin descriptions",
            "terminal functions",
            "pin list",
        ],
    ),
    ("absolute_maximum_ratings", &["absolute maximum"]),
    (
        "recommended_operating_conditions",
        &["recommended operating"],
    ),
    (
        "electrical_characteristics",
        &[
            "electrical characteristics",
            "dc characteristics",
            "electrical specifications",
        ],
    ),
    (
        "typical_application",
        &[
            "typical application",
            "application circuit",
            "application information",
            "applications information",
            "reference design",
            "typical circuit",
            "schematic",
        ],
    ),
    (
        "layout_guidelines",
        &[
            "layout guideline",
            "layout guidelines",
            "layout recommendation",
            "pcb layout",
            "layout considerations",
            "layout example",
            "board layout",
            "layout",
        ],
    ),
    (
        "thermal",
        &[
            "thermal information",
            "thermal resistance",
            "thermal characteristics",
            "thermal considerations",
        ],
    ),
    (
        "package",
        &[
            "package information",
            "mechanical data",
            "package outline",
            "package dimensions",
            "land pattern",
            "recommended footprint",
            "package drawing",
            "mechanical drawing",
        ],
    ),
    (
        "ordering",
        &[
            "ordering information",
            "order information",
            "part numbering",
        ],
    ),
];

fn clean(page: &str) -> String {
    let mut out = String::new();
    let mut blank = 0;
    for line in page.lines() {
        let l = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if l.is_empty() {
            blank += 1;
            if blank == 1 && !out.is_empty() {
                out.push('\n');
            }
            continue;
        }
        blank = 0;
        out.push_str(&l);
        out.push('\n');
    }
    out
}

fn is_toc_line(line: &str) -> bool {
    line.contains("....") || line.contains(". . .") || {
        let t = line.trim_end();
        let digits = t.chars().rev().take_while(|c| c.is_ascii_digit()).count();
        let head = &t[..t.len() - digits];
        let leading = t
            .trim_start_matches(|c: char| c.is_ascii_digit() || c == '.')
            .len()
            < t.len();
        digits > 0
            && digits <= 3
            && head.ends_with([' ', '.'])
            && head.trim().len() > 3
            && t.split_whitespace().count() <= 10
            && (leading || head.trim_end().ends_with('.'))
    }
}

fn is_toc_page(page: &str) -> bool {
    let lower = page.to_ascii_lowercase();
    let toc_lines = page.lines().filter(|l| is_toc_line(l)).count();
    lower.contains("table of contents")
        || lower.lines().any(|l| l.trim() == "contents")
        || toc_lines >= 5
}

fn heading_match(line: &str, key: &str) -> bool {
    let l = line.to_ascii_lowercase();
    let t = l.trim_start_matches(|c: char| c.is_ascii_digit() || c == '.' || c == ' ');
    l.len() <= 90
        && (t.starts_with(key) || (key.len() > 8 && t.contains(key) && t.len() <= key.len() + 30))
        && !is_toc_line(line)
}

pub fn find_sections(pages: &[String], excerpt_chars: usize) -> Vec<Value> {
    let mut out = Vec::new();
    for (name, keys) in SECTIONS {
        let mut hit = None;
        'outer: for key in *keys {
            for (i, page) in pages.iter().enumerate() {
                if is_toc_page(page) {
                    continue;
                }
                let lines: Vec<&str> = page.lines().collect();
                for (j, line) in lines.iter().enumerate() {
                    if heading_match(line, key) {
                        hit = Some((i, j));
                        break 'outer;
                    }
                }
            }
        }
        let Some((i, j)) = hit else { continue };
        let mut text = String::new();
        let mut last_page = i;
        for (k, page) in pages.iter().enumerate().skip(i) {
            let body: Vec<&str> = page.lines().skip(if k == i { j } else { 0 }).collect();
            for l in body {
                text.push_str(l);
                text.push('\n');
            }
            last_page = k;
            if text.len() >= excerpt_chars || k > i + 1 {
                break;
            }
        }
        let truncated = text.len() > excerpt_chars;
        if truncated {
            let mut cut = excerpt_chars;
            while !text.is_char_boundary(cut) {
                cut -= 1;
            }
            text.truncate(cut);
        }
        out.push(json!({ "section": name, "page": i + 1, "through_page": last_page + 1, "heading": pages[i].lines().nth(j).unwrap_or(""), "excerpt": text, "truncated": truncated }));
    }
    out
}

pub fn search(pages: &[String], query: &str, context: usize, max_hits: usize) -> Vec<Value> {
    let terms: Vec<String> = query
        .split('|')
        .map(|t| t.trim().to_ascii_lowercase())
        .filter(|t| !t.is_empty())
        .collect();
    let mut hits = Vec::new();
    for (i, page) in pages.iter().enumerate() {
        let lines: Vec<&str> = page.lines().collect();
        let mut last_end = 0usize;
        for (j, line) in lines.iter().enumerate() {
            let l = line.to_ascii_lowercase();
            let Some(term) = terms.iter().find(|t| l.contains(t.as_str())) else {
                continue;
            };
            let start = j.saturating_sub(context).max(last_end);
            let end = (j + context + 1).min(lines.len());
            if start >= end {
                continue;
            }
            last_end = end;
            hits.push(json!({ "page": i + 1, "term": term, "text": lines[start..end].join("\n") }));
            if hits.len() >= max_hits {
                return hits;
            }
        }
    }
    hits
}

pub fn extract_pages(bytes: &[u8]) -> Result<Vec<String>, String> {
    if !bytes.starts_with(b"%PDF") {
        let head = String::from_utf8_lossy(&bytes[..bytes.len().min(200)]).to_ascii_lowercase();
        return Err(if head.contains("<html") || head.contains("<!doctype") {
            "the URL returned a web page, not a PDF".into()
        } else {
            "not a PDF file".into()
        });
    }
    let owned = bytes.to_vec();
    let result =
        std::panic::catch_unwind(move || pdf_extract::extract_text_from_mem_by_pages(&owned));
    match result {
        Ok(Ok(pages)) => Ok(pages.iter().map(|p| clean(p)).collect()),
        Ok(Err(e)) => Err(format!("text extraction failed: {e}")),
        Err(_) => Err("text extraction crashed on this PDF".into()),
    }
}

fn cache_dir(project: Option<&str>) -> PathBuf {
    match project {
        Some(p) => {
            let p = PathBuf::from(p);
            let dir = if p.is_dir() {
                p
            } else {
                p.parent().map(Path::to_path_buf).unwrap_or(p)
            };
            dir.join(".voltaris").join("datasheets")
        }
        None => dirs::cache_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("voltaris")
            .join("datasheets"),
    }
}

fn safe_name(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .take(40)
        .collect()
}

async fn download(url: &str, dest: &Path) -> Result<Vec<u8>, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) Voltaris")
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("download failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("download failed: HTTP {}", resp.status()));
    }
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| format!("download failed: {e}"))?
        .to_vec();
    if bytes.len() > 60 * 1024 * 1024 {
        return Err("PDF is larger than 60 MB".into());
    }
    if bytes.starts_with(b"%PDF") {
        if let Some(parent) = dest.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(dest, &bytes);
    }
    Ok(bytes)
}

fn result_text(r: &CallToolResult) -> String {
    r.content
        .iter()
        .filter_map(|c| {
            if let ToolContent::Text { text } = c {
                Some(text.as_str())
            } else {
                None
            }
        })
        .collect()
}

pub fn tools() -> Vec<ToolDef> {
    vec![tool!(
        "read_datasheet",
        "Read a part's PDF datasheet. Give a local path, a URL, or an MPN / LCSC number (its \
         datasheet URL is looked up). The PDF is cached, its text extracted page by page, and the \
         result lists the key sections found — pin description, absolute maximum ratings, recommended \
         operating conditions, electrical characteristics, typical application, layout guidelines, \
         thermal, package, ordering — each with its page number and an excerpt. Add 'query' \
         ('|'-separated terms, e.g. \"decoupling|bypass|capacitor\") for page-cited matches, or \
         'pages' (\"3\" or \"5-7\") for full page text. Table layout and figures do not survive \
         extraction: cite the page and confirm critical numbers there.",
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Local PDF file" },
                "url": { "type": "string", "description": "Datasheet URL" },
                "mpn": { "type": "string" },
                "lcsc_id": { "type": "string" },
                "project": { "type": "string", "description": "Cache inside this project's .voltaris/datasheets instead of the user cache" },
                "query": { "type": "string" },
                "pages": { "type": "string" },
                "sections": { "type": "boolean", "default": true },
                "excerpt_chars": { "type": "integer", "minimum": 200, "maximum": 8000, "default": 1500 },
                "max_hits": { "type": "integer", "minimum": 1, "maximum": 60, "default": 15 }
            }
        }),
        |args, ctx| async move { handle_read_datasheet(args, ctx).await }
    )]
}

fn parse_pages(spec: &str, n: usize) -> Result<Vec<usize>, String> {
    let mut out = Vec::new();
    for part in spec.split(',') {
        let part = part.trim();
        let (a, b) = match part.split_once('-') {
            Some((a, b)) => (a.trim().parse::<usize>(), b.trim().parse::<usize>()),
            None => (part.parse::<usize>(), part.parse::<usize>()),
        };
        let (Ok(a), Ok(b)) = (a, b) else {
            return Err(format!("'{part}' is not a page or range"));
        };
        if a == 0 || b < a || b > n {
            return Err(format!("pages must be within 1-{n}"));
        }
        out.extend(a..=b);
    }
    out.dedup();
    if out.len() > 10 {
        return Err("at most 10 pages per call".into());
    }
    Ok(out)
}

async fn handle_read_datasheet(args: &Value, ctx: &ToolContext) -> anyhow::Result<CallToolResult> {
    let project = opt_str(args, "project");
    let cache = cache_dir(project);
    let (bytes, source, cached_path) = if let Some(path) = opt_str(args, "path") {
        match std::fs::read(path) {
            Ok(b) => (b, path.to_string(), PathBuf::from(path)),
            Err(e) => return Ok(invalid_arg("path", &e.to_string())),
        }
    } else {
        let url = match opt_str(args, "url") {
            Some(u) => u.to_string(),
            None => {
                if opt_str(args, "mpn").is_none() && opt_str(args, "lcsc_id").is_none() {
                    return Ok(CallToolResult::error(
                        "Give one of path, url, mpn or lcsc_id",
                    ));
                }
                let r = crate::tools::integration::handle_get_datasheet_url(args, ctx).await?;
                let v: Value = serde_json::from_str(&result_text(&r)).unwrap_or(Value::Null);
                match v["datasheet_url"].as_str() {
                    Some(u) if !u.is_empty() => u.to_string(),
                    _ => {
                        return Ok(CallToolResult::error(format!(
                            "No datasheet URL found for this part. Lookup said: {}",
                            result_text(&r)
                        )))
                    }
                }
            }
        };
        let url = if url.starts_with("//") {
            format!("https:{url}")
        } else {
            url
        };
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Ok(invalid_arg("url", "must be an http(s) URL"));
        }
        let hash = format!("{:x}", Sha256::digest(url.as_bytes()));
        let label = opt_str(args, "mpn")
            .or(opt_str(args, "lcsc_id"))
            .map(safe_name)
            .unwrap_or_else(|| "datasheet".into());
        let dest = cache.join(format!("{label}-{}.pdf", &hash[..12]));
        if dest.is_file() {
            (std::fs::read(&dest)?, url, dest)
        } else {
            match download(&url, &dest).await {
                Ok(b) => (b, url, dest),
                Err(e) => {
                    return Ok(CallToolResult::error(format!(
                        "{e}. URL: {url} — open it in a browser, save the PDF and pass its path."
                    )))
                }
            }
        }
    };
    let pages = match tokio::task::spawn_blocking(move || extract_pages(&bytes)).await? {
        Ok(p) => p,
        Err(e) => return Ok(CallToolResult::error(format!("{e} ({source})"))),
    };
    let chars: usize = pages.iter().map(String::len).sum();
    let mut out = json!({
        "source": source,
        "pdf": cached_path.display().to_string(),
        "page_count": pages.len(),
        "text_chars": chars,
    });
    if chars < 200 * pages.len().max(1) / 4 {
        out["warning"] = json!("Very little text was extracted — this PDF is probably scanned images. Read it visually instead.");
    }
    let excerpt = args["excerpt_chars"].as_u64().unwrap_or(1500) as usize;
    if args["sections"].as_bool().unwrap_or(true) {
        let s = find_sections(&pages, excerpt);
        let missing: Vec<&str> = SECTIONS
            .iter()
            .map(|(n, _)| *n)
            .filter(|n| !s.iter().any(|x| x["section"] == *n))
            .collect();
        out["sections"] = json!(s);
        out["sections_not_found"] = json!(missing);
    }
    if let Some(q) = opt_str(args, "query") {
        let max = args["max_hits"].as_u64().unwrap_or(15) as usize;
        out["matches"] = json!(search(&pages, q, 2, max));
    }
    if let Some(spec) = opt_str(args, "pages") {
        match parse_pages(spec, pages.len()) {
            Ok(list) => {
                out["pages"] = json!(list
                    .iter()
                    .map(|&p| json!({ "page": p, "text": pages[p - 1] }))
                    .collect::<Vec<_>>());
            }
            Err(e) => return Ok(invalid_arg("pages", &e)),
        }
    }
    out["note"] = json!("Cite page numbers when you use a value. Tables lose their columns in text form: confirm limits and pin numbers on the cited page.");
    Ok(CallToolResult::json(&out))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pages() -> Vec<String> {
        vec![
            "XYZ123 3A Buck Converter\nFeatures\n• 4.5 V to 17 V input\n".into(),
            "Table of Contents\n1 Features .......... 1\n2 Pin Configuration and Functions .......... 3\n6 Layout .......... 20\n".into(),
            "5 Pin Configuration and Functions\nPin Functions\nPIN NAME NO. DESCRIPTION\nGND 1 Ground\nSW 2 Switch node\nVIN 3 Input supply. Place a 10 µF ceramic capacitor close to VIN and GND.\n".into(),
            "6 Specifications\n6.1 Absolute Maximum Ratings\nVIN -0.3 19 V\nSW -1 20 V\n6.3 Recommended Operating Conditions\nVIN 4.5 17 V\n".into(),
            "10 Layout\n10.1 Layout Guidelines\nKeep the input capacitor loop short.\nUse a solid ground plane under the device.\n".into(),
        ]
    }

    #[test]
    fn sections_skip_the_contents_page() {
        let s = find_sections(&pages(), 300);
        let get = |n: &str| {
            s.iter()
                .find(|x| x["section"] == n)
                .cloned()
                .unwrap_or(Value::Null)
        };
        assert_eq!(get("pin_description")["page"], 3);
        assert_eq!(get("absolute_maximum_ratings")["page"], 4);
        assert_eq!(get("recommended_operating_conditions")["page"], 4);
        assert_eq!(get("layout_guidelines")["page"], 5);
        assert!(get("pin_description")["excerpt"]
            .as_str()
            .unwrap()
            .contains("Switch node"));
        assert!(is_toc_page(&pages()[1]));
        assert!(is_toc_line("1 Pin Configuration and Functions 2"));
        assert!(is_toc_line("2.1 Absolute Maximum Ratings . . . . . 4"));
        assert!(!is_toc_line("5 Pin Configuration and Functions"));
        assert!(!is_toc_line("VIN 3 Input supply 4.5 V to 17"));
    }

    #[test]
    fn query_hits_cite_pages_without_overlap() {
        let h = search(&pages(), "capacitor|ground plane", 1, 10);
        assert_eq!(
            h.iter()
                .map(|x| x["page"].as_u64().unwrap())
                .collect::<Vec<_>>(),
            vec![3, 5]
        );
        assert_eq!(parse_pages("2-4,5", 5).unwrap(), vec![2, 3, 4, 5]);
        assert!(parse_pages("0", 5).is_err() && parse_pages("4-9", 5).is_err());
    }

    #[test]
    fn rejects_non_pdf_and_survives_odd_pdfs() {
        assert!(extract_pages(b"<!DOCTYPE html><html>")
            .unwrap_err()
            .contains("web page"));
        assert!(extract_pages(b"%PDF-1.4 garbage").is_err());
    }
}
