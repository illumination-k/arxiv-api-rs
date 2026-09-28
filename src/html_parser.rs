use scraper::{node::Node, ElementRef, Html, Selector};

/// A fully parsed arXiv HTML paper.
#[derive(Debug, Clone)]
pub struct ArxivPaper {
    pub title: String,
    pub authors: Vec<String>,
    pub abstract_text: String,
    pub sections: Vec<Section>,
    /// Figures, tables and images placed before the first section (teaser
    /// figures, header images), in document order.
    pub leading_floats: Vec<ContentBlock>,
    /// Figures and tables outside any section after the first one — typically
    /// floats LaTeX emitted after the bibliography.
    pub trailing_floats: Vec<ContentBlock>,
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
    /// For floats without any image or table (TikZ boxes, algorithms) this holds
    /// the float's text as paragraphs.
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
    /// Images inside the table float, e.g. when the table is rendered as a picture
    /// or cells contain images.
    pub images: Vec<Image>,
    /// Sub-tables / sub-figures that carry their own caption. For tables drawn
    /// without `tabular` (e.g. TikZ boxes) this holds the text as paragraphs.
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

/// Alt texts LaTeXML puts on images; they carry no information.
const PLACEHOLDER_ALTS: [&str; 2] = ["Refer to caption", "[Uncaptioned image]"];

impl ArxivPaper {
    /// Parse an arXiv HTML page into a structured [`ArxivPaper`].
    pub fn parse(html: &str) -> Self {
        let document = Html::parse_document(html);

        let title = extract_title(&document);
        let authors = extract_authors(&document);
        let abstract_text = extract_abstract(&document);
        let sections = extract_sections(&document);
        let (leading_floats, trailing_floats) = extract_floats_outside_sections(&document);
        let references = extract_references(&document);

        Self {
            title,
            authors,
            abstract_text,
            sections,
            leading_floats,
            trailing_floats,
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
        let blocks = self
            .leading_floats
            .iter_mut()
            .chain(self.sections.iter_mut().flat_map(|s| &mut s.body))
            .chain(&mut self.trailing_floats);
        for block in blocks {
            block.for_each_image_mut(&mut |img| {
                if let Ok(resolved) = base.join(&img.src) {
                    img.src = resolved.to_string();
                }
            });
        }
    }

    /// All top-level blocks in document order, including floats outside sections.
    fn blocks(&self) -> impl Iterator<Item = &ContentBlock> {
        self.leading_floats
            .iter()
            .chain(self.sections.iter().flat_map(|s| &s.body))
            .chain(&self.trailing_floats)
    }

    /// Iterate over all figures in the paper (top-level floats only).
    pub fn figures(&self) -> impl Iterator<Item = &Figure> {
        self.blocks().filter_map(|b| match b {
            ContentBlock::Figure(f) => Some(f),
            _ => None,
        })
    }

    /// Iterate over all tables in the paper (top-level floats only).
    pub fn tables(&self) -> impl Iterator<Item = &Table> {
        self.blocks().filter_map(|b| match b {
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

        for block in &self.leading_floats {
            block.write_markdown(&mut md);
        }

        // Sections
        for section in &self.sections {
            section_to_markdown(section, &mut md);
        }

        for block in &self.trailing_floats {
            block.write_markdown(&mut md);
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
        self.expand(false)
    }

    /// Like [`Self::to_grid`], but with `fill_spans` every position a spanning
    /// cell covers repeats its text.
    fn expand(&self, fill_spans: bool) -> Vec<Vec<String>> {
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
                        *slot = Some(if fill_spans || (dr == 0 && dc == 0) {
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
    ///
    /// Rows marked as headers win. Otherwise the first row is the header, extended
    /// downwards while it groups columns (`colspan`) or has cells spanning into the
    /// next row — the usual shape of `\multicolumn` headers written with plain cells.
    fn header_row_count(&self) -> usize {
        let marked = self.rows.iter().take_while(|r| r.is_header).count();
        if marked > 0 {
            return marked;
        }
        let mut n = 1;
        while n < self.rows.len().saturating_sub(1) {
            let groups_columns = self.rows[n - 1].cells.iter().any(|c| c.colspan > 1);
            let spans_down = self.rows[..n]
                .iter()
                .enumerate()
                .any(|(r, row)| row.cells.iter().any(|c| r + c.rowspan > n));
            if !(groups_columns || spans_down) {
                break;
            }
            n += 1;
        }
        n
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
            // Header cells repeat spanning labels ("Score BLEU", "Score PPL").
            let n_header = self.header_row_count().max(1);
            let filled = self.expand(true);
            let header: Vec<String> = (0..width)
                .map(|c| {
                    let mut parts: Vec<&str> = Vec::new();
                    for row in &filled[..n_header] {
                        let text = row[c].as_str();
                        if !text.is_empty() && parts.last() != Some(&text) {
                            parts.push(text);
                        }
                    }
                    parts.join(" ")
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
    /// The alt text, unless it is empty or one of LaTeXML's generic placeholders.
    pub fn meaningful_alt(&self) -> Option<&str> {
        self.alt
            .as_deref()
            .map(str::trim)
            .filter(|a| !a.is_empty() && !PLACEHOLDER_ALTS.contains(a))
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

fn is_image(el: ElementRef) -> bool {
    matches!(el.value().name(), "img" | "object")
}

fn is_caption(el: ElementRef) -> bool {
    el.value().name() == "figcaption" || has_class(el, "ltx_caption")
}

/// Elements that start a new paragraph when they appear in running text.
fn is_block(el: ElementRef) -> bool {
    matches!(
        el.value().name(),
        "p" | "div"
            | "section"
            | "article"
            | "figure"
            | "table"
            | "ul"
            | "ol"
            | "li"
            | "dl"
            | "dt"
            | "dd"
            | "blockquote"
            | "pre"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
    )
}

/// Sections collected by [`extract_sections`].
fn is_collected_section(el: ElementRef) -> bool {
    el.value().name() == "section"
        && [
            "ltx_section",
            "ltx_subsection",
            "ltx_subsubsection",
            "ltx_appendix",
        ]
        .iter()
        .any(|c| has_class(el, c))
}

/// Whether the element is, or contains, content that is not plain text.
fn contains_structure(el: ElementRef) -> bool {
    el.descendants()
        .filter_map(ElementRef::wrap)
        .any(|d| is_float(d) || is_equation(d) || is_tabular(d) || is_image(d))
}

fn contains_image(el: ElementRef) -> bool {
    el.descendants().filter_map(ElementRef::wrap).any(is_image)
}

/// An image outside of any float, as an uncaptioned [`Figure`].
fn uncaptioned_figure(img: ElementRef) -> Option<ContentBlock> {
    parse_image(img).map(|image| {
        ContentBlock::Figure(Figure {
            id: img.value().attr("id").map(String::from),
            images: vec![image],
            ..Default::default()
        })
    })
}

/// Collect floats and images inside the document but outside every section,
/// split into those before the first section and those after it.
fn extract_floats_outside_sections(document: &Html) -> (Vec<ContentBlock>, Vec<ContentBlock>) {
    let mut leading = Vec::new();
    let mut trailing = Vec::new();
    let mut seen_section = false;

    for el in document
        .root_element()
        .descendants()
        .filter_map(ElementRef::wrap)
    {
        if is_collected_section(el) {
            seen_section = true;
            continue;
        }
        let candidate = is_float(el) || (is_image(el) && has_class(el, "ltx_graphics"));
        if !candidate {
            continue;
        }
        let mut in_document = false;
        let mut nested = false;
        for a in el.ancestors().filter_map(ElementRef::wrap) {
            if is_float(a) || is_collected_section(a) {
                nested = true;
                break;
            }
            in_document |= has_class(a, "ltx_document");
        }
        if nested || !in_document {
            continue;
        }
        let block = if is_float(el) {
            Some(parse_float(el))
        } else {
            uncaptioned_figure(el)
        };
        let target = if seen_section {
            &mut trailing
        } else {
            &mut leading
        };
        target.extend(block);
    }

    (leading, trailing)
}

/// Walk the children of a section / paragraph container, emitting content blocks
/// in document order. Nested sections are skipped (they are collected separately),
/// except `\paragraph{}` sections which are inlined.
fn extract_blocks(container: ElementRef, blocks: &mut Vec<ContentBlock>) {
    let mut sink = BlockSink {
        blocks,
        text: String::new(),
        pending: Vec::new(),
    };
    sink.children(container);
    sink.flush();
}

/// Accumulates inline content until a block boundary, so the text around an
/// inline table or image (`<p>before <table>…</table> after</p>`) is kept.
struct BlockSink<'b> {
    blocks: &'b mut Vec<ContentBlock>,
    /// Running text of the current paragraph.
    text: String,
    /// Inline images, emitted after the paragraph they appear in.
    pending: Vec<ContentBlock>,
}

impl BlockSink<'_> {
    /// Emit the running text as a paragraph, followed by its inline images.
    fn flush(&mut self) {
        let cleaned = clean_text(&self.text);
        if !cleaned.is_empty() {
            self.blocks.push(ContentBlock::Paragraph(cleaned));
        }
        self.text.clear();
        self.blocks.append(&mut self.pending);
    }

    fn push(&mut self, block: ContentBlock) {
        self.flush();
        self.blocks.push(block);
    }

    fn children(&mut self, container: ElementRef) {
        for node in container.children() {
            match ElementRef::wrap(node) {
                Some(el) => self.element(el),
                None => {
                    if let Node::Text(t) = node.value() {
                        self.text.push_str(t);
                    }
                }
            }
        }
    }

    fn element(&mut self, child: ElementRef) {
        let name = child.value().name();

        if is_caption(child) {
            return;
        }
        if is_heading(child) {
            // Section titles are handled by `extract_sections`; run-in titles of
            // theorems, proofs, etc. are kept as bold text.
            let in_section = child
                .parent()
                .and_then(ElementRef::wrap)
                .is_some_and(|p| p.value().name() == "section");
            if !in_section {
                let title = element_to_text(child);
                if !title.is_empty() {
                    self.push(ContentBlock::Paragraph(format!("**{title}**")));
                }
            }
            return;
        }
        if name == "section" {
            self.flush();
            if has_class(child, "ltx_paragraph") {
                let title = extract_section_title(child);
                if !title.is_empty() {
                    self.blocks
                        .push(ContentBlock::Paragraph(format!("**{title}**")));
                }
                self.children(child);
                self.flush();
            }
            return;
        }

        if is_float(child) {
            self.push(parse_float(child));
        } else if is_equation(child) {
            let latex = extract_equation(child);
            self.flush();
            if !latex.is_empty() {
                self.blocks.push(ContentBlock::Equation(latex));
            }
        } else if is_tabular(child) {
            let table = Table {
                id: child.value().attr("id").map(String::from),
                rows: parse_tabular(child),
                ..Default::default()
            };
            self.flush();
            if !table.rows.is_empty() {
                self.blocks.push(ContentBlock::Table(table));
            }
        } else if is_image(child) {
            self.pending.extend(uncaptioned_figure(child));
        } else if name == "li" {
            self.list_item(child);
        } else if contains_structure(child) || has_class(child, "ltx_listing") {
            // Listings are recursed into so each `ltx_listingline` is its own paragraph.
            let block = is_block(child);
            if block {
                self.flush();
            }
            self.children(child);
            if block {
                self.flush();
            }
        } else if is_block(child) {
            self.flush();
            let para = element_to_text(child);
            if !para.is_empty() {
                self.blocks.push(ContentBlock::Paragraph(para));
            }
        } else {
            collect_element(child, &mut self.text, false);
        }
    }

    /// A list item becomes its blocks, with the first paragraph prefixed by a
    /// Markdown bullet (`- `) or the item's own number (`1.`, `(a)`).
    fn list_item(&mut self, li: ElementRef) {
        self.flush();
        let mut marker = None;
        let start = self.blocks.len();
        for node in li.children() {
            match ElementRef::wrap(node) {
                Some(el) if marker.is_none() && has_class(el, "ltx_tag") => {
                    marker = Some(element_to_text(el));
                }
                Some(el) => self.element(el),
                None => {
                    if let Node::Text(t) = node.value() {
                        self.text.push_str(t);
                    }
                }
            }
        }
        self.flush();

        let marker = match marker.as_deref() {
            None | Some("•" | "∙" | "◦" | "–" | "-" | "") => "-".to_string(),
            Some(m) => m.to_string(),
        };
        match self.blocks.get_mut(start) {
            Some(ContentBlock::Paragraph(text)) => *text = format!("{marker} {text}"),
            _ => self.blocks.insert(
                start.min(self.blocks.len()),
                ContentBlock::Paragraph(marker),
            ),
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
            Node::Element(_) => {
                if let Some(child_el) = ElementRef::wrap(child) {
                    collect_element(child_el, out, skip_tags);
                }
            }
            _ => {}
        }
    }
}

/// Append the text of a single element (see [`collect_text`]).
fn collect_element(el: ElementRef, out: &mut String, skip_tags: bool) {
    let elem = el.value();
    match elem.name() {
        "math" => {
            if let Some(alt) = elem.attr("alttext") {
                out.push('$');
                out.push_str(alt);
                out.push('$');
            } else {
                collect_text(el, out, skip_tags);
            }
        }
        "br" => out.push(' '),
        "img" | "object" | "script" | "style" => {}
        _ if skip_tags && has_class(el, "ltx_tag") => {}
        _ => {
            collect_text(el, out, skip_tags);
            // Keep adjacent cells / blocks from running together.
            if matches!(elem.name(), "td" | "th" | "tr" | "p" | "div" | "li")
                || is_cell(el)
                || has_class(el, "ltx_tr")
                || has_class(el, "ltx_p")
                || has_class(el, "ltx_tag_item")
            {
                out.push(' ');
            }
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

    let table_class = has_class(el, "ltx_table");

    walk_float(el, &mut |d| {
        if is_float(d) {
            panels.push(parse_float(d));
            false
        } else if is_caption(d) {
            caption_el.get_or_insert(d);
            false
        } else if is_tabular(d) {
            // In a figure, a tabular holding images is a layout grid
            // (`\begin{tabular}` of `\includegraphics`): look inside it.
            if !table_class && contains_image(d) {
                return true;
            }
            // Images in table cells (icons, qualitative examples) are kept too.
            if table_class {
                images.extend(
                    d.descendants()
                        .filter_map(ElementRef::wrap)
                        .filter(|e| is_image(*e))
                        .filter_map(parse_image),
                );
            }
            tabulars.push(d);
            false
        } else if is_image(d) {
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
    let is_table =
        table_class || (images.is_empty() && (!tabulars.is_empty() || panels_are_tables));

    // Nothing structured inside (TikZ box, algorithm listing): keep the text.
    if images.is_empty() && tabulars.is_empty() && panels.is_empty() {
        extract_blocks(el, &mut panels);
    }

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
        // No marked header: the colspan/rowspan in the first row pull the second
        // row into the header, spanning labels are repeated, empty rows are
        // dropped and pipes are escaped.
        assert_eq!(
            t.to_markdown(),
            "**Table 2:** Spans.\n\n\
             | Model | Score BLEU | Score PPL |\n\
             | --- | --- | --- |\n\
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

    fn srcs(images: Vec<&Image>) -> Vec<&str> {
        images.into_iter().map(|i| i.src.as_str()).collect()
    }

    fn paragraphs(blocks: &[ContentBlock]) -> Vec<&str> {
        blocks
            .iter()
            .filter_map(|b| match b {
                ContentBlock::Paragraph(t) => Some(t.as_str()),
                _ => None,
            })
            .collect()
    }

    // ---- html_image_grids.html --------------------------------------------

    #[test]
    fn test_image_grid_tabular_is_a_figure() {
        let paper = parse("html_image_grids.html");
        let fig = figure(find_block(&paper, "S1.F1"));
        assert_eq!(fig.label.as_deref(), Some("Figure 1"));
        assert_eq!(
            srcs(fig.images.iter().collect()),
            vec![
                "2304.00001v1/grid/a1.jpg",
                "2304.00001v1/grid/a2.jpg",
                "2304.00001v1/grid/b1.jpg",
                "2304.00001v1/grid/b2.jpg",
            ]
        );
        assert!(fig.panels.is_empty(), "layout grid must not become a table");
        assert_eq!(fig.images[0].meaningful_alt(), None);
    }

    #[test]
    fn test_image_grid_inside_subfigure() {
        let paper = parse("html_image_grids.html");
        let fig = figure(find_block(&paper, "S1.F2"));
        assert!(fig.images.is_empty());
        assert_eq!(fig.panels.len(), 1);
        let sub = figure(&fig.panels[0]);
        assert_eq!(sub.label.as_deref(), Some("(a)"));
        assert_eq!(
            srcs(sub.images.iter().collect()),
            vec!["2304.00001v1/masks/m1.jpg", "2304.00001v1/masks/m2.jpg"]
        );
        assert_eq!(fig.all_images().len(), 2);
    }

    #[test]
    fn test_table_float_keeps_cell_images() {
        let paper = parse("html_image_grids.html");
        let t = table(find_block(&paper, "S1.T1"));
        assert_eq!(t.to_grid(), vec![vec!["Model", "Logo"], vec!["SAM", ""]]);
        assert_eq!(
            srcs(t.images.iter().collect()),
            vec!["2304.00001v1/icon.png"]
        );
        assert!(t.to_markdown().contains("![icon](2304.00001v1/icon.png)"));
    }

    #[test]
    fn test_image_inside_svg_foreign_object() {
        let paper = parse("html_image_grids.html");
        let fig = figure(find_block(&paper, "S1.F3"));
        assert_eq!(
            srcs(fig.images.iter().collect()),
            vec!["2304.00001v1/tikz_inner.png"]
        );
    }

    #[test]
    fn test_uncaptioned_placeholder_alt_is_not_used() {
        let paper = parse("html_image_grids.html");
        let md = paper.to_markdown();
        assert!(!md.contains("Uncaptioned"), "{md}");
        assert!(md.contains("![Figure 1](2304.00001v1/grid/a1.jpg)"));
    }

    // ---- html_outside_sections.html ---------------------------------------

    #[test]
    fn test_leading_floats_before_first_section() {
        let paper = parse("html_outside_sections.html");
        assert_eq!(paper.leading_floats.len(), 2);

        let header = figure(&paper.leading_floats[0]);
        assert_eq!(header.id.as_deref(), Some("g1"));
        assert_eq!(header.label, None);
        assert_eq!(header.caption, "");
        assert_eq!(header.images[0].src, "2401.00004v1/images/header.jpeg");
        assert_eq!(header.images[0].width, Some(381));

        let teaser = figure(&paper.leading_floats[1]);
        assert_eq!(teaser.label.as_deref(), Some("Figure 1"));
        assert_eq!(teaser.caption, "Teaser.");
    }

    #[test]
    fn test_trailing_floats_after_bibliography() {
        let paper = parse("html_outside_sections.html");
        assert_eq!(paper.trailing_floats.len(), 2);
        assert_eq!(
            figure(&paper.trailing_floats[0]).label.as_deref(),
            Some("Figure 3")
        );
        let t = table(&paper.trailing_floats[1]);
        assert_eq!(t.label.as_deref(), Some("Table 1"));
        assert_eq!(t.to_grid(), vec![vec!["a", "b"], vec!["1", "2"]]);

        // The in-section figure is not duplicated.
        assert_eq!(paper.sections[0].body.len(), 2);
    }

    #[test]
    fn test_floats_outside_sections_in_iterators_and_markdown() {
        let mut paper = parse("html_outside_sections.html");
        let labels: Vec<_> = paper.figures().map(|f| f.label.clone()).collect();
        assert_eq!(
            labels,
            vec![
                None,
                Some("Figure 1".to_string()),
                Some("Figure 2".to_string()),
                Some("Figure 3".to_string()),
            ]
        );
        assert_eq!(paper.tables().count(), 1);

        let md = paper.to_markdown();
        let pos = |needle: &str| md.find(needle).unwrap_or_else(|| panic!("{needle}: {md}"));
        assert!(pos("## Abstract") < pos("**Figure 1:** Teaser."));
        assert!(pos("**Figure 1:** Teaser.") < pos("## 1 Introduction"));
        assert!(pos("**Figure 2:**") < pos("**Figure 3:**"));
        assert!(pos("**Table 1:** A late table.") < pos("## References"));
        assert!(!md.contains("/static/") && !md.contains("[LOGO]"), "{md}");

        paper.resolve_image_urls(&url::Url::parse("https://arxiv.org/html/2401.00004v1").unwrap());
        assert_eq!(
            figure(&paper.leading_floats[1]).images[0].src,
            "https://arxiv.org/html/2401.00004v1/teaser.png"
        );
        assert_eq!(
            figure(&paper.trailing_floats[0]).images[0].src,
            "https://arxiv.org/html/2401.00004v1/late_figure.png"
        );
    }

    // ---- html_text_floats.html --------------------------------------------

    #[test]
    fn test_algorithm_float_lines() {
        let paper = parse("html_text_floats.html");
        let alg = figure(find_block(&paper, "alg1"));
        assert_eq!(alg.label.as_deref(), Some("Algorithm 1"));
        assert_eq!(alg.caption, "Self-Rag Inference");
        assert!(alg.images.is_empty());
        assert_eq!(
            paragraphs(&alg.panels),
            vec![
                r"1: Generator LM $\mathcal{M}$",
                "2: if Retrieve == Yes then",
                "3: Retrieve passages",
            ]
        );
    }

    #[test]
    fn test_tikz_box_table_keeps_text() {
        let paper = parse("html_text_floats.html");
        let t = table(find_block(&paper, "A4.T8"));
        assert_eq!(t.label.as_deref(), Some("Table 8"));
        assert!(t.rows.is_empty() && t.images.is_empty());
        assert_eq!(
            paragraphs(&t.panels),
            vec!["Instructions Given an instruction, decide whether retrieval helps."]
        );
        assert_eq!(
            t.to_markdown(),
            "**Table 8:** Instructions and demonstrations.\n\n\
             Instructions Given an instruction, decide whether retrieval helps.\n\n"
        );
    }

    #[test]
    fn test_tikz_figure_labels_do_not_run_together() {
        let paper = parse("html_text_floats.html");
        let fig = figure(find_block(&paper, "S1.F1"));
        assert_eq!(paragraphs(&fig.panels), vec!["Encoder Decoder"]);
    }

    // ---- html_inline_content.html -----------------------------------------

    #[test]
    fn test_text_around_inline_tabular_is_kept() {
        let paper = parse("html_inline_content.html");
        let body = &paper.sections[0].body;
        assert!(matches!(&body[0], ContentBlock::Paragraph(t) if t == "Before the table $x$,"));
        assert_eq!(
            table(&body[1]).to_grid(),
            vec![vec!["k", "v"], vec!["1", "2"]]
        );
        assert!(matches!(&body[2], ContentBlock::Paragraph(t) if t == "and after it."));
    }

    #[test]
    fn test_inline_image_does_not_split_paragraph() {
        let paper = parse("html_inline_content.html");
        let body = &paper.sections[0].body;
        assert!(
            matches!(&body[3], ContentBlock::Paragraph(t) if t == "Press the button to start."),
            "{:?}",
            body[3]
        );
        let icon = figure(&body[4]);
        assert_eq!(icon.images[0].src, "2401.00005v1/icon.png");
        assert_eq!(icon.images[0].meaningful_alt(), Some("button"));
    }

    #[test]
    fn test_list_items_and_figure_in_item() {
        let paper = parse("html_inline_content.html");
        let body = &paper.sections[0].body;
        assert!(matches!(&body[5], ContentBlock::Paragraph(t) if t == "- First item."));
        assert!(matches!(&body[6], ContentBlock::Paragraph(t) if t == "- Second item."));
        assert_eq!(figure(&body[7]).label.as_deref(), Some("Figure 1"));
    }

    #[test]
    fn test_theorem_title_and_equation() {
        let paper = parse("html_inline_content.html");
        let body = &paper.sections[0].body;
        assert!(matches!(&body[8], ContentBlock::Paragraph(t) if t == "**Theorem 1.**"));
        assert!(matches!(&body[9], ContentBlock::Paragraph(t) if t == "For all $n$:"));
        assert!(matches!(&body[10], ContentBlock::Equation(l) if l == "n+0=n"));
    }

    #[test]
    fn test_multilevel_header_without_thead() {
        let paper = parse("html_inline_content.html");
        let t = table(find_block(&paper, "S1.T1"));
        assert!(t.rows.iter().all(|r| !r.is_header));
        assert_eq!(t.rows.len(), 5, "tfoot rows are included");
        assert_eq!(
            t.to_markdown(),
            "**Table 1:** Accuracy by length.\n\n\
             | Model | Length $2^{6}$ | Length $2^{7}$ | Length $2^{8}$ |\n\
             | --- | --- | --- | --- |\n\
             | Mamba | 100.0 | 100.0 | 99.8 |\n\
             | MHA | 99.6 | ✗ | ✗ |\n\
             | ✗: out of memory |  |  |  |\n\n"
        );
    }

    #[test]
    fn test_header_heuristic_keeps_a_body_row() {
        // A single row with a colspan must not swallow the whole table.
        let t = Table {
            rows: vec![
                row(false, vec![cell("Group", false, 2, 1)]),
                row(false, vec![cell("a", false, 1, 1), cell("b", false, 1, 1)]),
            ],
            ..Default::default()
        };
        assert_eq!(
            t.to_markdown(),
            "| Group | Group |\n| --- | --- |\n| a | b |\n\n"
        );
    }
}
