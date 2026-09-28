use scraper::{node::Node, ElementRef, Html, Selector};

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
    Figure(Figure),
    Table(Table),
    Equation(String),
}

/// A figure float (`<figure class="ltx_figure">`).
#[derive(Debug, Clone, Default)]
pub struct Figure {
    pub id: Option<String>,
    /// Numbering label such as `"Figure 1"`, taken from the caption tag.
    pub label: Option<String>,
    /// Caption text without the label. Inline math is rendered as `$...$`.
    pub caption: String,
    /// Images belonging directly to this figure (not to its panels).
    pub images: Vec<Image>,
    /// Sub-figures / sub-tables that carry their own caption (e.g. `(a)`, `(b)`).
    pub panels: Vec<ContentBlock>,
}

/// A table float (`<figure class="ltx_table">`) or a bare `ltx_tabular`.
#[derive(Debug, Clone, Default)]
pub struct Table {
    pub id: Option<String>,
    /// Numbering label such as `"Table 1"`, taken from the caption tag.
    pub label: Option<String>,
    /// Caption text without the label. Inline math is rendered as `$...$`.
    pub caption: String,
    /// Rows of the table as they appear in the HTML (spans are not expanded).
    pub rows: Vec<TableRow>,
    /// Images inside the table float, e.g. when the table is rendered as a picture.
    pub images: Vec<Image>,
    /// Sub-tables / sub-figures that carry their own caption.
    pub panels: Vec<ContentBlock>,
}

/// A single row of a [`Table`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TableRow {
    /// `true` when the row is part of `<thead>` or consists only of column headers.
    pub is_header: bool,
    pub cells: Vec<TableCell>,
}

/// A single cell of a [`TableRow`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableCell {
    /// Cell text. Inline math is rendered as `$...$`.
    pub text: String,
    /// `true` for `<th>` cells (row or column headers).
    pub is_header: bool,
    pub colspan: usize,
    pub rowspan: usize,
}

/// An image referenced by a figure or table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    /// Image URL. Relative to the paper page unless resolved with
    /// [`ArxivPaper::resolve_image_urls`].
    pub src: String,
    pub alt: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

/// A bibliography entry.
#[derive(Debug, Clone)]
pub struct Reference {
    pub id: String,
    pub label: String,
    pub text: String,
}

/// Maximum span honoured when expanding `colspan`/`rowspan`, to guard against
/// pathological input.
const MAX_SPAN: usize = 1000;

/// Alt text LaTeXML puts on every image; carries no information.
const PLACEHOLDER_ALT: &str = "Refer to caption";

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

    /// Resolve every relative image `src` against `base` (the URL of the paper page,
    /// e.g. `https://arxiv.org/html/2310.06825v1`).
    pub fn resolve_image_urls(&mut self, base: &url::Url) {
        // arXiv image paths already contain the paper id (`2310.06825v1/x1.png`),
        // so they must be resolved against `/html/`, not `/html/<id>/`.
        let mut base = base.clone();
        if base.path().ends_with('/') && base.path().len() > 1 {
            let trimmed = base.path().trim_end_matches('/').to_string();
            base.set_path(&trimmed);
        }
        for section in &mut self.sections {
            for block in &mut section.body {
                block.for_each_image_mut(&mut |img| {
                    if let Ok(resolved) = base.join(&img.src) {
                        img.src = resolved.to_string();
                    }
                });
            }
        }
    }

    /// Iterate over all figures in the paper (top-level floats only).
    pub fn figures(&self) -> impl Iterator<Item = &Figure> {
        self.sections
            .iter()
            .flat_map(|s| &s.body)
            .filter_map(|b| match b {
                ContentBlock::Figure(f) => Some(f),
                _ => None,
            })
    }

    /// Iterate over all tables in the paper (top-level floats only).
    pub fn tables(&self) -> impl Iterator<Item = &Table> {
        self.sections
            .iter()
            .flat_map(|s| &s.body)
            .filter_map(|b| match b {
                ContentBlock::Table(t) => Some(t),
                _ => None,
            })
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

impl ContentBlock {
    fn for_each_image_mut(&mut self, f: &mut dyn FnMut(&mut Image)) {
        let (images, panels) = match self {
            ContentBlock::Figure(fig) => (&mut fig.images, &mut fig.panels),
            ContentBlock::Table(table) => (&mut table.images, &mut table.panels),
            _ => return,
        };
        images.iter_mut().for_each(&mut *f);
        for panel in panels {
            panel.for_each_image_mut(f);
        }
    }

    fn write_markdown(&self, md: &mut String) {
        match self {
            ContentBlock::Paragraph(text) => {
                md.push_str(text);
                md.push_str("\n\n");
            }
            ContentBlock::Figure(figure) => figure.write_markdown(md),
            ContentBlock::Table(table) => table.write_markdown(md),
            ContentBlock::Equation(latex) => {
                md.push_str(&format!("$$\n{latex}\n$$\n\n"));
            }
        }
    }
}

impl Figure {
    /// Every image of this figure, including those of its panels.
    pub fn all_images(&self) -> Vec<&Image> {
        let mut out: Vec<&Image> = self.images.iter().collect();
        for panel in &self.panels {
            match panel {
                ContentBlock::Figure(f) => out.extend(f.all_images()),
                ContentBlock::Table(t) => out.extend(&t.images),
                _ => {}
            }
        }
        out
    }

    /// Render the figure as Markdown: images, panels, then the caption.
    pub fn to_markdown(&self) -> String {
        let mut md = String::new();
        self.write_markdown(&mut md);
        md
    }

    fn write_markdown(&self, md: &mut String) {
        let fallback_alt = self.label.as_deref().unwrap_or("Figure");
        for image in &self.images {
            md.push_str(&format!(
                "![{}]({})\n\n",
                escape_alt(image.meaningful_alt().unwrap_or(fallback_alt)),
                image.src
            ));
        }
        for panel in &self.panels {
            panel.write_markdown(md);
        }
        write_caption(md, self.label.as_deref(), &self.caption);
    }
}

impl Table {
    /// Expand `colspan`/`rowspan` into a rectangular grid of cell texts.
    ///
    /// A spanning cell's text is placed in its top-left position; the other
    /// positions it covers are empty strings.
    pub fn to_grid(&self) -> Vec<Vec<String>> {
        let mut grid: Vec<Vec<Option<String>>> = vec![Vec::new(); self.rows.len()];

        for (r, row) in self.rows.iter().enumerate() {
            let mut c = 0;
            for cell in &row.cells {
                while grid[r].get(c).is_some_and(Option::is_some) {
                    c += 1;
                }
                let colspan = cell.colspan.clamp(1, MAX_SPAN);
                let rowspan = cell.rowspan.clamp(1, MAX_SPAN).min(self.rows.len() - r);
                for (dr, grid_row) in grid[r..r + rowspan].iter_mut().enumerate() {
                    if grid_row.len() < c + colspan {
                        grid_row.resize(c + colspan, None);
                    }
                    for (dc, slot) in grid_row[c..c + colspan].iter_mut().enumerate() {
                        *slot = Some(if dr == 0 && dc == 0 {
                            cell.text.clone()
                        } else {
                            String::new()
                        });
                    }
                }
                c += colspan;
            }
        }

        let width = grid.iter().map(Vec::len).max().unwrap_or(0);
        grid.into_iter()
            .map(|row| {
                let mut row: Vec<String> = row.into_iter().map(Option::unwrap_or_default).collect();
                row.resize(width, String::new());
                row
            })
            .collect()
    }

    /// Number of leading rows that form the table header.
    fn header_row_count(&self) -> usize {
        self.rows.iter().take_while(|r| r.is_header).count()
    }

    /// Render the table as Markdown: caption, a GFM table, images, then panels.
    pub fn to_markdown(&self) -> String {
        let mut md = String::new();
        self.write_markdown(&mut md);
        md
    }

    fn write_markdown(&self, md: &mut String) {
        write_caption(md, self.label.as_deref(), &self.caption);

        let grid = self.to_grid();
        let width = grid.first().map_or(0, Vec::len);
        if width > 0 {
            // GFM tables have exactly one header row: merge multi-row headers
            // column-wise, and fall back to the first row when none is marked.
            let n_header = self.header_row_count().max(1);
            let header: Vec<String> = (0..width)
                .map(|c| {
                    grid[..n_header]
                        .iter()
                        .map(|row| row[c].as_str())
                        .filter(|s| !s.is_empty())
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .collect();

            write_table_row(md, &header);
            md.push('|');
            md.push_str(&" --- |".repeat(width));
            md.push('\n');
            for row in &grid[n_header..] {
                if row.iter().all(String::is_empty) {
                    continue;
                }
                write_table_row(md, row);
            }
            md.push('\n');
        }

        let fallback_alt = self.label.as_deref().unwrap_or("Table");
        for image in &self.images {
            md.push_str(&format!(
                "![{}]({})\n\n",
                escape_alt(image.meaningful_alt().unwrap_or(fallback_alt)),
                image.src
            ));
        }
        for panel in &self.panels {
            panel.write_markdown(md);
        }
    }
}

impl Image {
    /// The alt text, unless it is empty or LaTeXML's generic placeholder.
    pub fn meaningful_alt(&self) -> Option<&str> {
        self.alt
            .as_deref()
            .map(str::trim)
            .filter(|a| !a.is_empty() && *a != PLACEHOLDER_ALT)
    }
}

fn section_to_markdown(section: &Section, md: &mut String) {
    let prefix = "#".repeat(section.level as usize + 1);
    md.push_str(&format!("{prefix} {}\n\n", section.title));

    for block in &section.body {
        block.write_markdown(md);
    }
}

fn write_caption(md: &mut String, label: Option<&str>, caption: &str) {
    match (label, caption.is_empty()) {
        (Some(label), false) => md.push_str(&format!("**{label}:** {caption}\n\n")),
        (Some(label), true) => md.push_str(&format!("**{label}**\n\n")),
        (None, false) => md.push_str(&format!("*{caption}*\n\n")),
        (None, true) => {}
    }
}

fn write_table_row(md: &mut String, cells: &[String]) {
    md.push('|');
    for cell in cells {
        md.push(' ');
        md.push_str(&cell.replace('|', "\\|"));
        md.push_str(" |");
    }
    md.push('\n');
}

fn escape_alt(alt: &str) -> String {
    alt.replace('[', "\\[").replace(']', "\\]")
}

// ---------------------------------------------------------------------------
// Extraction helpers
// ---------------------------------------------------------------------------

fn select_one<'a>(document: &'a Html, selector: &str) -> Option<ElementRef<'a>> {
    let sel = Selector::parse(selector).ok()?;
    document.select(&sel).next()
}

fn has_class(el: ElementRef, class: &str) -> bool {
    el.value().classes().any(|c| c == class)
}

fn child_elements<'a>(el: ElementRef<'a>) -> impl Iterator<Item = ElementRef<'a>> {
    el.children().filter_map(ElementRef::wrap)
}

fn extract_title(document: &Html) -> String {
    select_one(document, "h1.ltx_title_document")
        .map(element_to_text)
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
            .map(element_to_text)
            .collect::<Vec<_>>()
            .join("\n")
    })
}

fn extract_sections(document: &Html) -> Vec<Section> {
    let mut sections = Vec::new();

    // Collect top-level sections, subsections, and subsubsections in document order.
    let sel = Selector::parse("section.ltx_section, section.ltx_subsection, section.ltx_subsubsection, section.ltx_appendix").unwrap();

    for section_el in document.select(&sel) {
        // Skip the bibliography section — handled separately.
        if has_class(section_el, "ltx_bibliography") {
            continue;
        }

        let level = if has_class(section_el, "ltx_subsection") {
            2
        } else if has_class(section_el, "ltx_subsubsection") {
            3
        } else {
            1
        };

        let id = section_el.value().attr("id").map(String::from);
        let title = extract_section_title(section_el);

        let mut body = Vec::new();
        extract_blocks(section_el, &mut body);

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
    child_elements(section)
        .find(|el| matches!(el.value().name(), "h2" | "h3" | "h4" | "h5" | "h6"))
        .map(element_to_text)
        .unwrap_or_default()
}

fn is_heading(el: ElementRef) -> bool {
    matches!(el.value().name(), "h1" | "h2" | "h3" | "h4" | "h5" | "h6")
}

fn is_equation(el: ElementRef) -> bool {
    el.value().name() == "table"
        && (has_class(el, "ltx_equation")
            || has_class(el, "ltx_equationgroup")
            || has_class(el, "ltx_eqn_table"))
}

fn is_tabular(el: ElementRef) -> bool {
    has_class(el, "ltx_tabular")
}

fn is_float(el: ElementRef) -> bool {
    el.value().name() == "figure"
}

/// Whether the element is, or contains, content that is not plain text.
fn contains_structure(el: ElementRef) -> bool {
    el.descendants()
        .filter_map(ElementRef::wrap)
        .any(|d| is_float(d) || is_equation(d) || is_tabular(d))
}

/// Walk the children of a section / paragraph container, emitting content blocks
/// in document order. Nested sections are skipped (they are collected separately),
/// except `\paragraph{}` sections which are inlined.
fn extract_blocks(container: ElementRef, blocks: &mut Vec<ContentBlock>) {
    for child in child_elements(container) {
        let name = child.value().name();

        if is_heading(child) {
            continue;
        }
        if name == "section" {
            if has_class(child, "ltx_paragraph") {
                let title = extract_section_title(child);
                if !title.is_empty() {
                    blocks.push(ContentBlock::Paragraph(format!("**{title}**")));
                }
                extract_blocks(child, blocks);
            }
            continue;
        }
        if is_float(child) {
            blocks.push(parse_float(child));
        } else if is_equation(child) {
            let latex = extract_equation(child);
            if !latex.is_empty() {
                blocks.push(ContentBlock::Equation(latex));
            }
        } else if is_tabular(child) {
            let table = Table {
                id: child.value().attr("id").map(String::from),
                rows: parse_tabular(child),
                ..Default::default()
            };
            if !table.rows.is_empty() {
                blocks.push(ContentBlock::Table(table));
            }
        } else if contains_structure(child) {
            extract_blocks(child, blocks);
        } else {
            let text = element_to_text(child);
            if !text.is_empty() {
                blocks.push(ContentBlock::Paragraph(text));
            }
        }
    }
}

/// Convert an element's content to text, replacing `<math>` with LaTeX from `alttext`,
/// and `<cite>` with its text content.
fn element_to_text(el: ElementRef) -> String {
    let mut out = String::new();
    collect_text(el, &mut out, false);
    clean_text(&out)
}

/// Like [`element_to_text`] but drops `.ltx_tag` elements (e.g. `"Figure 1: "`).
fn element_to_text_without_tag(el: ElementRef) -> String {
    let mut out = String::new();
    collect_text(el, &mut out, true);
    clean_text(&out)
}

fn collect_text(el: ElementRef, out: &mut String, skip_tags: bool) {
    for child in el.children() {
        match child.value() {
            Node::Text(text) => out.push_str(text),
            Node::Element(elem) => {
                let Some(child_el) = ElementRef::wrap(child) else {
                    continue;
                };
                match elem.name() {
                    "math" => {
                        if let Some(alt) = elem.attr("alttext") {
                            out.push('$');
                            out.push_str(alt);
                            out.push('$');
                        } else {
                            collect_text(child_el, out, skip_tags);
                        }
                    }
                    "br" => out.push(' '),
                    "img" | "object" | "script" | "style" => {}
                    _ if skip_tags && has_class(child_el, "ltx_tag") => {}
                    _ => {
                        collect_text(child_el, out, skip_tags);
                        // Keep adjacent cells / blocks from running together.
                        if matches!(elem.name(), "td" | "th" | "tr" | "p" | "div" | "li")
                            || is_cell(child_el)
                            || has_class(child_el, "ltx_tr")
                        {
                            out.push(' ');
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

/// Visit descendants of a float in document order without entering nested floats.
/// `visit` returns whether to descend into the given element.
fn walk_float<'a>(el: ElementRef<'a>, visit: &mut dyn FnMut(ElementRef<'a>) -> bool) {
    for child in child_elements(el) {
        if visit(child) && !is_float(child) {
            walk_float(child, visit);
        }
    }
}

/// Parse a `<figure>` element into a [`ContentBlock::Figure`] or [`ContentBlock::Table`].
fn parse_float(el: ElementRef) -> ContentBlock {
    let id = el.value().attr("id").map(String::from);

    let mut caption_el = None;
    let mut images = Vec::new();
    let mut tabulars = Vec::new();
    let mut panels = Vec::new();

    walk_float(el, &mut |d| {
        if is_float(d) {
            panels.push(parse_float(d));
            false
        } else if d.value().name() == "figcaption" || has_class(d, "ltx_caption") {
            caption_el.get_or_insert(d);
            false
        } else if is_tabular(d) {
            tabulars.push(d);
            false
        } else if matches!(d.value().name(), "img" | "object") {
            if let Some(image) = parse_image(d) {
                images.push(image);
            }
            false
        } else {
            true
        }
    });

    let (label, caption) = caption_el.map_or((None, String::new()), parse_caption);

    // Wrap-figures often hold a table inside `<figure class="ltx_figure">`;
    // classify by content when the class says "figure" but there is no image.
    let panels_are_tables =
        !panels.is_empty() && panels.iter().all(|p| matches!(p, ContentBlock::Table(_)));
    let is_table = has_class(el, "ltx_table")
        || (images.is_empty() && (!tabulars.is_empty() || panels_are_tables));

    if !is_table {
        // Tables that live directly inside a figure become panels.
        for t in tabulars {
            panels.push(ContentBlock::Table(Table {
                id: t.value().attr("id").map(String::from),
                rows: parse_tabular(t),
                ..Default::default()
            }));
        }
        return ContentBlock::Figure(Figure {
            id,
            label,
            caption,
            images,
            panels,
        });
    }

    let mut tabulars = tabulars.into_iter();
    let rows = tabulars.next().map(parse_tabular).unwrap_or_default();
    // Several side-by-side tabulars under one caption: keep the extras as panels.
    let extra: Vec<ContentBlock> = tabulars
        .map(|t| {
            ContentBlock::Table(Table {
                id: t.value().attr("id").map(String::from),
                rows: parse_tabular(t),
                ..Default::default()
            })
        })
        .collect();
    panels.splice(0..0, extra);

    ContentBlock::Table(Table {
        id,
        label,
        caption,
        rows,
        images,
        panels,
    })
}

/// Split a caption into its label (`"Figure 1"`) and the remaining text.
fn parse_caption(caption: ElementRef) -> (Option<String>, String) {
    let label = caption
        .descendants()
        .filter_map(ElementRef::wrap)
        .find(|d| has_class(*d, "ltx_tag"))
        .map(|tag| {
            element_to_text(tag)
                .trim_end_matches([':', '.'])
                .trim()
                .to_string()
        })
        .filter(|l| !l.is_empty());
    (label, element_to_text_without_tag(caption))
}

/// Parse an `<img src>` or an `<object data>` (used by LaTeXML for SVG graphics).
fn parse_image(img: ElementRef) -> Option<Image> {
    let attr = |name| img.value().attr(name);
    let src_attr = if img.value().name() == "object" {
        "data"
    } else {
        "src"
    };
    let src = attr(src_attr).map(str::trim).filter(|s| !s.is_empty())?;
    Some(Image {
        src: src.to_string(),
        alt: attr("alt").map(String::from),
        width: attr("width").and_then(|w| w.trim().parse().ok()),
        height: attr("height").and_then(|h| h.trim().parse().ok()),
    })
}

/// Parse the rows of an `ltx_tabular` (either `<table>` or the `<span>` variant).
fn parse_tabular(tabular: ElementRef) -> Vec<TableRow> {
    let mut rows = Vec::new();
    collect_rows(tabular, false, &mut rows);
    rows
}

fn collect_rows(el: ElementRef, in_head: bool, rows: &mut Vec<TableRow>) {
    for child in child_elements(el) {
        let name = child.value().name();
        if name == "tr" || has_class(child, "ltx_tr") {
            rows.push(parse_row(child, in_head));
        } else if name == "thead" || has_class(child, "ltx_thead") {
            collect_rows(child, true, rows);
        } else if !is_tabular(child) && !is_cell(child) {
            // tbody / tfoot / wrappers
            collect_rows(child, in_head, rows);
        }
    }
}

fn is_cell(el: ElementRef) -> bool {
    matches!(el.value().name(), "td" | "th") || has_class(el, "ltx_td")
}

fn parse_row(tr: ElementRef, in_head: bool) -> TableRow {
    let mut all_column_headers = true;
    let mut any_text = false;

    let cells: Vec<TableCell> = child_elements(tr)
        .filter(|c| is_cell(*c))
        .map(|c| {
            let text = element_to_text(c);
            let is_header = c.value().name() == "th" || has_class(c, "ltx_th");
            if !text.is_empty() {
                any_text = true;
                // Row headers (`ltx_th_row`) label a row, not the column.
                let column_header =
                    is_header && !has_class(c, "ltx_th_row") || has_class(c, "ltx_th_column");
                all_column_headers &= column_header;
            }
            let span = |name| {
                c.value()
                    .attr(name)
                    .and_then(|v| v.trim().parse::<usize>().ok())
                    .unwrap_or(1)
                    .max(1)
            };
            TableCell {
                text,
                is_header,
                colspan: span("colspan"),
                rowspan: span("rowspan"),
            }
        })
        .collect();

    TableRow {
        is_header: in_head || (any_text && all_column_headers),
        cells,
    }
}

/// Extract the LaTeX of a display equation. Multi-line groups (`align`,
/// `eqnarray`) are joined with `\\`.
fn extract_equation(el: ElementRef) -> String {
    let math_sel = Selector::parse("math").unwrap();
    let tr_sel = Selector::parse("tr").unwrap();

    let lines: Vec<String> = el
        .select(&tr_sel)
        .map(|row| {
            row.select(&math_sel)
                .filter_map(|m| m.value().attr("alttext"))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .filter(|l| !l.is_empty())
        .collect();

    if lines.is_empty() {
        // Not a table-based layout: fall back to the first math element.
        return el
            .select(&math_sel)
            .next()
            .and_then(|m| m.value().attr("alttext").map(String::from))
            .unwrap_or_default();
    }
    lines.join(" \\\\\n")
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
                .map(element_to_text)
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

    fn load_fixture(name: &str) -> String {
        let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("Failed to read fixture {path}: {e}"))
    }

    fn parse(name: &str) -> ArxivPaper {
        ArxivPaper::parse(&load_fixture(name))
    }

    fn figure(block: &ContentBlock) -> &Figure {
        match block {
            ContentBlock::Figure(f) => f,
            other => panic!("expected Figure, got {other:?}"),
        }
    }

    fn table(block: &ContentBlock) -> &Table {
        match block {
            ContentBlock::Table(t) => t,
            other => panic!("expected Table, got {other:?}"),
        }
    }

    fn find_block<'a>(paper: &'a ArxivPaper, id: &str) -> &'a ContentBlock {
        paper
            .sections
            .iter()
            .flat_map(|s| &s.body)
            .find(|b| match b {
                ContentBlock::Figure(f) => f.id.as_deref() == Some(id),
                ContentBlock::Table(t) => t.id.as_deref() == Some(id),
                _ => false,
            })
            .unwrap_or_else(|| panic!("block {id} not found"))
    }

    fn cell(text: &str, is_header: bool, colspan: usize, rowspan: usize) -> TableCell {
        TableCell {
            text: text.to_string(),
            is_header,
            colspan,
            rowspan,
        }
    }

    fn row(is_header: bool, cells: Vec<TableCell>) -> TableRow {
        TableRow { is_header, cells }
    }

    // ---- sample_paper.html ------------------------------------------------

    #[test]
    fn test_parse_title() {
        let paper = parse("sample_paper.html");
        assert_eq!(paper.title, "Test Paper Title");
    }

    #[test]
    fn test_parse_authors() {
        let paper = parse("sample_paper.html");
        assert_eq!(paper.authors, vec!["Alice Smith", "Bob Jones"]);
    }

    #[test]
    fn test_parse_abstract() {
        let paper = parse("sample_paper.html");
        assert_eq!(paper.abstract_text, "This is the abstract text.");
    }

    #[test]
    fn test_parse_sections() {
        let paper = parse("sample_paper.html");
        assert_eq!(paper.sections.len(), 2);
        assert_eq!(paper.sections[0].title, "1 Introduction");
        assert_eq!(paper.sections[0].level, 1);
        assert_eq!(paper.sections[1].title, "2 Methods");
    }

    #[test]
    fn test_parse_section_body() {
        let paper = parse("sample_paper.html");
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
        let paper = parse("sample_paper.html");
        let fig = figure(&paper.sections[1].body[1]);
        assert_eq!(fig.id.as_deref(), Some("S2.F1"));
        assert_eq!(fig.label.as_deref(), Some("Figure 1"));
        assert_eq!(fig.caption, "A nice figure.");
        assert_eq!(fig.images.len(), 1);
        assert_eq!(fig.images[0].src, "x1.png");
        assert_eq!(fig.images[0].alt.as_deref(), Some("Figure 1"));
    }

    #[test]
    fn test_parse_references() {
        let paper = parse("sample_paper.html");
        assert_eq!(paper.references.len(), 2);

        assert_eq!(paper.references[0].id, "bib.bib1");
        assert_eq!(paper.references[0].label, "Smith (2024)");
        assert!(paper.references[0].text.contains("A great paper"));

        assert_eq!(paper.references[1].label, "Jones (2023)");
    }

    #[test]
    fn test_to_markdown() {
        let paper = parse("sample_paper.html");
        let md = paper.to_markdown();

        assert!(md.starts_with("# Test Paper Title\n"));
        assert!(md.contains("Alice Smith, Bob Jones"));
        assert!(md.contains("## Abstract"));
        assert!(md.contains("## 1 Introduction"));
        assert!(md.contains("## 2 Methods"));
        assert!(md.contains("![Figure 1](x1.png)\n\n**Figure 1:** A nice figure.\n\n"));
        assert!(md.contains("## References"));
        assert!(md.contains("**[Smith (2024)]**"));
    }

    // ---- html_figures.html ------------------------------------------------

    #[test]
    fn test_figure_single_image_with_math_caption() {
        let paper = parse("html_figures.html");
        let fig = figure(find_block(&paper, "S1.F1"));
        assert_eq!(fig.label.as_deref(), Some("Figure 1"));
        assert_eq!(
            fig.caption,
            "Sliding window. Each token attends to $W$ tokens."
        );
        assert_eq!(
            fig.images,
            vec![Image {
                src: "2401.00001v1/x1.png".to_string(),
                alt: Some("Refer to caption".to_string()),
                width: Some(471),
                height: Some(176),
            }]
        );
        assert_eq!(fig.images[0].meaningful_alt(), None);
        assert!(fig.panels.is_empty());
    }

    #[test]
    fn test_figure_flex_panels_without_captions() {
        let paper = parse("html_figures.html");
        let fig = figure(find_block(&paper, "S1.F2"));
        assert_eq!(fig.label.as_deref(), Some("Figure 2"));
        assert_eq!(fig.caption, "(left) Left thing. (right) Right thing.");
        let srcs: Vec<_> = fig.images.iter().map(|i| i.src.as_str()).collect();
        assert_eq!(
            srcs,
            vec![
                "2401.00001v1/Figures/left.png",
                "2401.00001v1/Figures/right.png"
            ]
        );
        assert!(fig.panels.is_empty());
    }

    #[test]
    fn test_figure_subfigures() {
        let paper = parse("html_figures.html");
        let fig = figure(find_block(&paper, "S1.F3"));
        assert_eq!(fig.label.as_deref(), Some("Figure 3"));
        assert_eq!(fig.caption, "Training curves.");
        assert!(fig.images.is_empty(), "images belong to the panels");
        assert_eq!(fig.panels.len(), 2);

        let a = figure(&fig.panels[0]);
        assert_eq!(a.id.as_deref(), Some("S1.F3.sf1"));
        assert_eq!(a.label.as_deref(), Some("(a)"));
        assert_eq!(a.caption, "Accuracy");
        assert_eq!(a.images[0].src, "2401.00001v1/a.png");
        assert_eq!(a.images[0].meaningful_alt(), Some("Accuracy curve"));

        let b = figure(&fig.panels[1]);
        assert_eq!(b.label.as_deref(), Some("(b)"));
        assert_eq!(b.caption, "Loss");

        let all: Vec<_> = fig.all_images().iter().map(|i| i.src.clone()).collect();
        assert_eq!(all, vec!["2401.00001v1/a.png", "2401.00001v1/b.png"]);

        let md = fig.to_markdown();
        assert_eq!(
            md,
            "![Accuracy curve](2401.00001v1/a.png)\n\n**(a):** Accuracy\n\n\
             ![(b)](2401.00001v1/b.png)\n\n**(b):** Loss\n\n\
             **Figure 3:** Training curves.\n\n"
        );
    }

    #[test]
    fn test_figure_svg_object() {
        let paper = parse("html_figures.html");
        let fig = figure(find_block(&paper, "S1.F4"));
        assert_eq!(fig.images.len(), 1);
        assert_eq!(fig.images[0].src, "2401.00001v1/diagram.svg");
        assert_eq!(fig.images[0].width, Some(476));
        assert_eq!(fig.images[0].alt, None);
    }

    #[test]
    fn test_figure_without_image_file() {
        let paper = parse("html_figures.html");
        let fig = figure(find_block(&paper, "S1.F5"));
        assert!(fig.images.is_empty());
        assert_eq!(fig.caption, "A TikZ picture.");
        assert_eq!(fig.to_markdown(), "**Figure 5:** A TikZ picture.\n\n");
    }

    #[test]
    fn test_figures_ignore_page_chrome_images() {
        let paper = parse("html_figures.html");
        assert_eq!(paper.figures().count(), 6);
        assert_eq!(paper.tables().count(), 0);
        let all: Vec<String> = paper
            .figures()
            .flat_map(|f| f.all_images())
            .map(|i| i.src.clone())
            .collect();
        assert!(all.iter().all(|s| !s.contains("/static/")), "{all:?}");
        assert_eq!(all.len(), 8);
    }

    #[test]
    fn test_resolve_image_urls() {
        let mut paper = parse("html_figures.html");
        let base = url::Url::parse("https://arxiv.org/html/2401.00001v1/").unwrap();
        paper.resolve_image_urls(&base);

        let srcs: Vec<String> = paper
            .figures()
            .flat_map(|f| f.all_images())
            .map(|i| i.src.clone())
            .collect();
        assert_eq!(srcs[0], "https://arxiv.org/html/2401.00001v1/x1.png");
        assert_eq!(
            srcs[1],
            "https://arxiv.org/html/2401.00001v1/Figures/left.png"
        );
        assert_eq!(srcs[4], "https://arxiv.org/html/2401.00001v1/b.png");
        assert_eq!(srcs[6], "https://example.com/abs.png");
        assert!(srcs[7].starts_with("data:image/png;base64,"));

        // Base without trailing slash resolves the same way.
        let mut paper = parse("html_figures.html");
        paper.resolve_image_urls(&url::Url::parse("https://arxiv.org/html/2401.00001v1").unwrap());
        assert_eq!(
            paper.figures().next().unwrap().images[0].src,
            "https://arxiv.org/html/2401.00001v1/x1.png"
        );
    }

    #[test]
    fn test_figure_markdown_alt_fallback() {
        let paper = parse("html_figures.html");
        let md = paper.to_markdown();
        assert!(md.contains("![Figure 1](2401.00001v1/x1.png)\n\n**Figure 1:** Sliding window."));
        assert!(md.contains("![inline](data:image/png;base64,iVBORw0KGgo=)"));
        // Figure without label or caption: generic alt, no caption line.
        assert!(md.contains("![Figure](https://example.com/abs.png)"));
    }

    // ---- html_tables.html -------------------------------------------------

    #[test]
    fn test_table_header_rows_and_math_cells() {
        let paper = parse("html_tables.html");
        let t = table(find_block(&paper, "S1.T1"));
        assert_eq!(t.label.as_deref(), Some("Table 1"));
        assert_eq!(t.caption, "Complexity. $n$ is the length.");
        assert_eq!(t.rows.len(), 4);
        assert_eq!(
            t.rows[0],
            row(
                true,
                vec![
                    cell("Layer Type", true, 1, 1),
                    cell("Complexity", true, 1, 1),
                    cell("Sequential", true, 1, 1),
                ]
            )
        );
        // Header continuation row inside <tbody>.
        assert!(t.rows[1].is_header);
        // Row headers (`ltx_th_row`) do not make a header row.
        assert!(!t.rows[2].is_header);
        assert_eq!(t.rows[2].cells[0], cell("Self-Attention", true, 1, 1));
        assert_eq!(t.rows[2].cells[1].text, r"$O(n^{2}\cdot d)$");

        assert_eq!(
            t.to_markdown(),
            "**Table 1:** Complexity. $n$ is the length.\n\n\
             | Layer Type | Complexity | Sequential Operations |\n\
             | --- | --- | --- |\n\
             | Self-Attention | $O(n^{2}\\cdot d)$ | $O(1)$ |\n\
             | Recurrent | $O(n\\cdot d^{2})$ | $O(n)$ |\n\n"
        );
    }

    #[test]
    fn test_table_spans_grid() {
        let paper = parse("html_tables.html");
        let t = table(find_block(&paper, "S1.T2"));
        assert_eq!(t.rows[0].cells[0], cell("Model", false, 1, 2));
        assert_eq!(t.rows[0].cells[1], cell("Score", false, 2, 1));
        assert!(t.rows.iter().all(|r| !r.is_header));

        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(
            t.to_grid(),
            vec![
                s(&["Model", "Score", ""]),
                s(&["", "BLEU", "PPL"]),
                s(&["", "", ""]),
                s(&["(A)", "25.8", "4.92"]),
                s(&["", "24.9", "a|b"]),
            ]
        );
    }

    #[test]
    fn test_table_spans_markdown() {
        let paper = parse("html_tables.html");
        let t = table(find_block(&paper, "S1.T2"));
        // No header row: the first row becomes the header; empty rows are
        // dropped and pipes are escaped.
        assert_eq!(
            t.to_markdown(),
            "**Table 2:** Spans.\n\n\
             | Model | Score |  |\n\
             | --- | --- | --- |\n\
             |  | BLEU | PPL |\n\
             | (A) | 25.8 | 4.92 |\n\
             |  | 24.9 | a\\|b |\n\n"
        );
    }

    #[test]
    fn test_table_with_subtables() {
        let paper = parse("html_tables.html");
        let t = table(find_block(&paper, "S1.T3"));
        assert_eq!(t.label, None);
        assert!(t.rows.is_empty());
        assert_eq!(t.panels.len(), 2);

        let left = table(&t.panels[0]);
        assert_eq!(left.label.as_deref(), Some("Table 3"));
        assert_eq!(left.caption, "Left sub-table");
        assert_eq!(
            left.to_grid(),
            vec![vec!["Threshold", "ROUGE-L"], vec!["1", "0.36"]]
        );

        let right = table(&t.panels[1]);
        assert_eq!(right.label.as_deref(), Some("Table 4"));
        assert_eq!(right.rows[1].cells[1].text, "1.5");
    }

    #[test]
    fn test_table_inside_figure_class() {
        let paper = parse("html_tables.html");
        let t = table(find_block(&paper, "S1.T5"));
        assert_eq!(t.label.as_deref(), Some("Table 5"));
        assert_eq!(t.caption, "Model architecture.");
        assert!(t.rows[0].is_header);
        assert_eq!(t.rows[1].cells[0].text, "dim");
        assert_eq!(t.rows[1].cells[1].text, "$4096$");
    }

    #[test]
    fn test_table_as_image() {
        let paper = parse("html_tables.html");
        let t = table(find_block(&paper, "S1.T6"));
        assert!(t.rows.is_empty());
        assert_eq!(t.images.len(), 1);
        assert_eq!(t.images[0].src, "2401.00002v1/table6.png");
        assert_eq!(
            t.to_markdown(),
            "**Table 6:** Screenshot table.\n\n![Table 6](2401.00002v1/table6.png)\n\n"
        );
    }

    #[test]
    fn test_table_span_tabular_and_nested_tabular() {
        let paper = parse("html_tables.html");
        let t = table(find_block(&paper, "S1.T7"));
        assert_eq!(t.rows.len(), 2, "nested tabular rows must not leak out");
        assert!(t.rows[0].is_header);
        assert_eq!(t.rows[1].cells.len(), 2);
        assert_eq!(t.rows[1].cells[1].text, "top bottom");
    }

    #[test]
    fn test_tables_iterator() {
        let paper = parse("html_tables.html");
        let labels: Vec<_> = paper.tables().map(|t| t.label.clone()).collect();
        assert_eq!(
            labels,
            vec![
                Some("Table 1".to_string()),
                Some("Table 2".to_string()),
                None,
                Some("Table 5".to_string()),
                Some("Table 6".to_string()),
                Some("Table 7".to_string()),
            ]
        );
        assert_eq!(paper.figures().count(), 0);
    }

    #[test]
    fn test_grid_handles_overlong_rowspan() {
        let t = Table {
            rows: vec![row(
                false,
                vec![cell("a", false, 1, 99), cell("b", false, 1, 1)],
            )],
            ..Default::default()
        };
        assert_eq!(t.to_grid(), vec![vec!["a", "b"]]);
    }

    #[test]
    fn test_ragged_rows_are_padded() {
        let t = Table {
            rows: vec![
                row(true, vec![cell("h1", true, 1, 1), cell("h2", true, 1, 1)]),
                row(false, vec![cell("only", false, 1, 1)]),
            ],
            ..Default::default()
        };
        assert_eq!(
            t.to_markdown(),
            "| h1 | h2 |\n| --- | --- |\n| only |  |\n\n"
        );
    }

    // ---- html_structure.html ----------------------------------------------

    #[test]
    fn test_equation_inside_paragraph_keeps_order() {
        let paper = parse("html_structure.html");
        let body = &paper.sections[0].body;
        assert!(matches!(&body[0], ContentBlock::Paragraph(t) if t == "We compute:"));
        assert!(matches!(&body[1], ContentBlock::Equation(l) if l == "E=mc^{2}"));
        assert!(matches!(&body[2], ContentBlock::Paragraph(t) if t == "where $c$ is light speed."));
    }

    #[test]
    fn test_equation_group_multiline() {
        let paper = parse("html_structure.html");
        let body = &paper.sections[0].body;
        assert!(
            matches!(&body[3], ContentBlock::Equation(l) if l == "a =b+c \\\\\n=d"),
            "{:?}",
            body[3]
        );
    }

    #[test]
    fn test_bare_tabular_and_figure_inside_paragraph() {
        let paper = parse("html_structure.html");
        let body = &paper.sections[0].body;
        assert!(matches!(&body[4], ContentBlock::Paragraph(t) if t == "Inline table follows."));
        let t = table(&body[5]);
        assert_eq!(t.label, None);
        assert_eq!(t.to_grid(), vec![vec!["k", "v"], vec!["1", "2"]]);

        let f = figure(&body[6]);
        assert_eq!(f.label.as_deref(), Some("Figure 1"));
        assert_eq!(f.images[0].src, "2401.00003v1/in_para.png");
    }

    #[test]
    fn test_paragraph_sections_inlined_and_subsections_separate() {
        let paper = parse("html_structure.html");
        let body = &paper.sections[0].body;
        assert!(matches!(&body[7], ContentBlock::Paragraph(t) if t == "**Setup.**"));
        assert!(matches!(&body[8], ContentBlock::Paragraph(t) if t == "Paragraph body."));
        assert_eq!(body.len(), 9, "subsection content must not leak: {body:?}");

        let titles: Vec<_> = paper
            .sections
            .iter()
            .map(|s| (s.title.as_str(), s.level))
            .collect();
        assert_eq!(
            titles,
            vec![("1 Body", 1), ("1.1 Sub", 2), ("Appendix A Extra", 1)]
        );
    }
}
