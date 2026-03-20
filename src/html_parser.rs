use scraper::{ElementRef, Html, Selector};

/// A fully parsed arXiv HTML paper.
#[derive(Debug, Clone)]
pub struct ArxivPaper {
    pub title: String,
    pub authors: Vec<String>,
    pub abstract_text: String,
    pub sections: Vec<Section>,
    pub references: Vec<Reference>,
}

/// A section (or subsection/subsubsection) of a paper.
#[derive(Debug, Clone)]
pub struct Section {
    pub id: Option<String>,
    pub title: String,
    /// Nesting level: 1 = section (h2), 2 = subsection (h3), 3 = subsubsection (h4).
    pub level: u8,
    pub body: Vec<ContentBlock>,
}

/// A block of content within a section.
#[derive(Debug, Clone)]
pub enum ContentBlock {
    Paragraph(String),
    Figure {
        caption: String,
        src: Option<String>,
    },
    Table {
        caption: String,
        content: String,
    },
    Equation(String),
}

/// A bibliography entry.
#[derive(Debug, Clone)]
pub struct Reference {
    pub id: String,
    pub label: String,
    pub text: String,
}

impl ArxivPaper {
    /// Parse an arXiv HTML page into a structured [`ArxivPaper`].
    pub fn parse(html: &str) -> Self {
        let document = Html::parse_document(html);

        let title = extract_title(&document);
        let authors = extract_authors(&document);
        let abstract_text = extract_abstract(&document);
        let sections = extract_sections(&document);
        let references = extract_references(&document);

        Self {
            title,
            authors,
            abstract_text,
            sections,
            references,
        }
    }

    /// Convert the parsed paper to Markdown.
    pub fn to_markdown(&self) -> String {
        let mut md = String::new();

        // Title
        md.push_str(&format!("# {}\n\n", self.title));

        // Authors
        if !self.authors.is_empty() {
            md.push_str(&self.authors.join(", "));
            md.push_str("\n\n");
        }

        // Abstract
        if !self.abstract_text.is_empty() {
            md.push_str("## Abstract\n\n");
            md.push_str(&self.abstract_text);
            md.push_str("\n\n");
        }

        // Sections
        for section in &self.sections {
            section_to_markdown(section, &mut md);
        }

        // References
        if !self.references.is_empty() {
            md.push_str("## References\n\n");
            for reference in &self.references {
                md.push_str(&format!("- **[{}]** {}\n", reference.label, reference.text));
            }
            md.push('\n');
        }

        md
    }
}

fn section_to_markdown(section: &Section, md: &mut String) {
    let prefix = "#".repeat(section.level as usize + 1);
    md.push_str(&format!("{prefix} {}\n\n", section.title));

    for block in &section.body {
        match block {
            ContentBlock::Paragraph(text) => {
                md.push_str(text);
                md.push_str("\n\n");
            }
            ContentBlock::Figure { caption, src } => {
                if let Some(src) = src {
                    md.push_str(&format!("![{caption}]({src})\n\n"));
                }
                if !caption.is_empty() {
                    md.push_str(&format!("*{caption}*\n\n"));
                }
            }
            ContentBlock::Table { caption, content } => {
                if !caption.is_empty() {
                    md.push_str(&format!("*{caption}*\n\n"));
                }
                md.push_str(content);
                md.push_str("\n\n");
            }
            ContentBlock::Equation(latex) => {
                md.push_str(&format!("$$\n{latex}\n$$\n\n"));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Extraction helpers
// ---------------------------------------------------------------------------

fn select_one<'a>(document: &'a Html, selector: &str) -> Option<ElementRef<'a>> {
    let sel = Selector::parse(selector).ok()?;
    document.select(&sel).next()
}

fn extract_title(document: &Html) -> String {
    select_one(document, "h1.ltx_title_document")
        .map(|el| clean_text(&el.text().collect::<String>()))
        .unwrap_or_default()
}

fn extract_authors(document: &Html) -> Vec<String> {
    let Some(sel) = Selector::parse("span.ltx_personname").ok() else {
        return Vec::new();
    };
    document
        .select(&sel)
        .map(|el| clean_text(&el.text().collect::<String>()))
        .filter(|s| !s.is_empty())
        .collect()
}

fn extract_abstract(document: &Html) -> String {
    select_one(document, "div.ltx_abstract").map_or_else(String::new, |el| {
        // The abstract div contains an h6 "Abstract" heading we want to skip.
        let sel_p = Selector::parse("p.ltx_p").unwrap();
        el.select(&sel_p)
            .map(|p| element_to_text(p))
            .collect::<Vec<_>>()
            .join("\n")
    })
}

fn extract_sections(document: &Html) -> Vec<Section> {
    let mut sections = Vec::new();

    // Collect top-level sections, subsections, and subsubsections in document order.
    let sel = Selector::parse("section.ltx_section, section.ltx_subsection, section.ltx_subsubsection, section.ltx_appendix").unwrap();

    for section_el in document.select(&sel) {
        let classes = section_el.value().attr("class").unwrap_or("");
        let level = if classes.contains("ltx_subsection") {
            2
        } else if classes.contains("ltx_subsubsection") {
            3
        } else {
            1
        };

        let id = section_el.value().attr("id").map(String::from);

        // Title is the first heading child.
        let title = extract_section_title(section_el);

        // Skip the bibliography section — handled separately.
        if classes.contains("ltx_bibliography") {
            continue;
        }

        let body = extract_section_body(section_el);

        sections.push(Section {
            id,
            title,
            level,
            body,
        });
    }

    sections
}

fn extract_section_title(section: ElementRef) -> String {
    let sel = Selector::parse("h2, h3, h4, h5, h6").unwrap();
    section
        .select(&sel)
        .next()
        .map(|el| clean_text(&el.text().collect::<String>()))
        .unwrap_or_default()
}

fn extract_section_body(section: ElementRef) -> Vec<ContentBlock> {
    let mut blocks = Vec::new();

    for child in section.children() {
        let Some(child_el) = ElementRef::wrap(child) else {
            continue;
        };

        let tag = child_el.value().name();
        let classes = child_el.value().attr("class").unwrap_or("");

        match tag {
            // Paragraph containers
            "div" if classes.contains("ltx_para") => {
                let text = extract_para_text(child_el);
                if !text.is_empty() {
                    blocks.push(ContentBlock::Paragraph(text));
                }
            }
            // Figures
            "figure" if classes.contains("ltx_figure") => {
                let (caption, src) = extract_figure(child_el);
                blocks.push(ContentBlock::Figure { caption, src });
            }
            // Tables (arXiv wraps tables in <figure class="ltx_table">)
            "figure" if classes.contains("ltx_table") => {
                let (caption, content) = extract_table(child_el);
                blocks.push(ContentBlock::Table { caption, content });
            }
            // Display equations
            "table" if classes.contains("ltx_equation") || classes.contains("ltx_eqn_table") => {
                let latex = extract_equation(child_el);
                if !latex.is_empty() {
                    blocks.push(ContentBlock::Equation(latex));
                }
            }
            _ => {}
        }
    }

    blocks
}

/// Extract text from a `div.ltx_para`, converting inline math to LaTeX.
fn extract_para_text(para: ElementRef) -> String {
    let mut parts = Vec::new();

    let sel_p = Selector::parse("p.ltx_p").unwrap();
    for p in para.select(&sel_p) {
        parts.push(element_to_text(p));
    }

    if parts.is_empty() {
        return element_to_text(para);
    }

    parts.join("\n")
}

/// Convert an element's content to text, replacing `<math>` with LaTeX from `alttext`,
/// and `<cite>` with its text content.
fn element_to_text(el: ElementRef) -> String {
    let mut out = String::new();
    collect_text(el, &mut out);
    clean_text(&out)
}

fn collect_text(el: ElementRef, out: &mut String) {
    use scraper::Node;

    for child in el.children() {
        match child.value() {
            Node::Text(text) => out.push_str(text),
            Node::Element(elem) => {
                if let Some(child_el) = ElementRef::wrap(child) {
                    match elem.name() {
                        "math" => {
                            if let Some(alt) = elem.attr("alttext") {
                                out.push('$');
                                out.push_str(alt);
                                out.push('$');
                            } else {
                                collect_text(child_el, out);
                            }
                        }
                        "cite" => {
                            collect_text(child_el, out);
                        }
                        "br" => {
                            out.push(' ');
                        }
                        _ => {
                            collect_text(child_el, out);
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

fn extract_figure(figure: ElementRef) -> (String, Option<String>) {
    let img_sel = Selector::parse("img").unwrap();
    let src = figure
        .select(&img_sel)
        .next()
        .and_then(|img| img.value().attr("src").map(String::from));

    let caption = extract_caption(figure);
    (caption, src)
}

fn extract_table(table_figure: ElementRef) -> (String, String) {
    let caption = extract_caption(table_figure);

    // Try to extract tabular content as plain text.
    let sel = Selector::parse("table.ltx_tabular").unwrap();
    let content = table_figure
        .select(&sel)
        .next()
        .map(|t| element_to_text(t))
        .unwrap_or_default();

    (caption, content)
}

fn extract_caption(el: ElementRef) -> String {
    let sel = Selector::parse(".ltx_caption").unwrap();
    el.select(&sel)
        .next()
        .map(|c| clean_text(&c.text().collect::<String>()))
        .unwrap_or_default()
}

fn extract_equation(el: ElementRef) -> String {
    let sel = Selector::parse("math").unwrap();
    el.select(&sel)
        .next()
        .and_then(|m| m.value().attr("alttext").map(String::from))
        .unwrap_or_default()
}

fn extract_references(document: &Html) -> Vec<Reference> {
    let Some(sel) = Selector::parse("li.ltx_bibitem").ok() else {
        return Vec::new();
    };

    document
        .select(&sel)
        .map(|item| {
            let id = item.value().attr("id").unwrap_or_default().to_string();

            let label_sel = Selector::parse(".ltx_tag_bibitem").unwrap();
            let label = item
                .select(&label_sel)
                .next()
                .map(|l| clean_text(&l.text().collect::<String>()))
                .unwrap_or_default();

            let block_sel = Selector::parse(".ltx_bibblock").unwrap();
            let text = item
                .select(&block_sel)
                .map(|b| clean_text(&b.text().collect::<String>()))
                .collect::<Vec<_>>()
                .join(" ");

            Reference { id, label, text }
        })
        .collect()
}

/// Collapse whitespace and trim.
fn clean_text(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod test {
    use super::*;

    fn load_fixture() -> String {
        let path = format!(
            "{}/tests/fixtures/sample_paper.html",
            env!("CARGO_MANIFEST_DIR")
        );
        std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("Failed to read fixture {path}: {e}"))
    }

    #[test]
    fn test_parse_title() {
        let paper = ArxivPaper::parse(&load_fixture());
        assert_eq!(paper.title, "Test Paper Title");
    }

    #[test]
    fn test_parse_authors() {
        let paper = ArxivPaper::parse(&load_fixture());
        assert_eq!(paper.authors, vec!["Alice Smith", "Bob Jones"]);
    }

    #[test]
    fn test_parse_abstract() {
        let paper = ArxivPaper::parse(&load_fixture());
        assert_eq!(paper.abstract_text, "This is the abstract text.");
    }

    #[test]
    fn test_parse_sections() {
        let paper = ArxivPaper::parse(&load_fixture());
        assert_eq!(paper.sections.len(), 2);
        assert_eq!(paper.sections[0].title, "1 Introduction");
        assert_eq!(paper.sections[0].level, 1);
        assert_eq!(paper.sections[1].title, "2 Methods");
    }

    #[test]
    fn test_parse_section_body() {
        let paper = ArxivPaper::parse(&load_fixture());
        let intro = &paper.sections[0];
        assert_eq!(intro.body.len(), 2);
        match &intro.body[0] {
            ContentBlock::Paragraph(text) => {
                assert!(text.contains("First paragraph"));
                assert!(text.contains("Smith (2024)"));
            }
            other => panic!("expected Paragraph, got {other:?}"),
        }
        match &intro.body[1] {
            ContentBlock::Paragraph(text) => {
                assert!(
                    text.contains(r"$\alpha + \beta$"),
                    "expected LaTeX math, got: {text}"
                );
            }
            other => panic!("expected Paragraph, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_figure() {
        let paper = ArxivPaper::parse(&load_fixture());
        let methods = &paper.sections[1];
        assert!(methods.body.iter().any(|b| matches!(
            b,
            ContentBlock::Figure { caption, src }
                if caption.contains("A nice figure")
                    && *src == Some("x1.png".to_string())
        )));
    }

    #[test]
    fn test_parse_references() {
        let paper = ArxivPaper::parse(&load_fixture());
        assert_eq!(paper.references.len(), 2);

        assert_eq!(paper.references[0].id, "bib.bib1");
        assert_eq!(paper.references[0].label, "Smith (2024)");
        assert!(paper.references[0].text.contains("A great paper"));

        assert_eq!(paper.references[1].label, "Jones (2023)");
    }

    #[test]
    fn test_to_markdown() {
        let paper = ArxivPaper::parse(&load_fixture());
        let md = paper.to_markdown();

        assert!(md.starts_with("# Test Paper Title\n"));
        assert!(md.contains("Alice Smith, Bob Jones"));
        assert!(md.contains("## Abstract"));
        assert!(md.contains("## 1 Introduction"));
        assert!(md.contains("## 2 Methods"));
        assert!(md.contains("## References"));
        assert!(md.contains("**[Smith (2024)]**"));
    }
}
