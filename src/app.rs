use crate::book::Book;
use directories::ProjectDirs;
use eframe::egui;
use epub::doc::EpubDoc;
use rfd::FileDialog;
use serde_json;
use std::path::{Path, PathBuf};

pub struct EbookApp {
    books: Vec<Book>,
    library_path: Option<PathBuf>,
    reading_book: Option<Book>,
    reading_content: String,
    reading_error: Option<String>,
    reader_current_page: usize,
    reader_font_size: f32,
    reader_search: String,
    show_library: bool,
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
#[serde(default)]
struct AppConfig {
    library_path: Option<PathBuf>,
    added_books: Vec<Book>,
    reading_book: Option<Book>,
    reading_content: String,
    reading_error: Option<String>,
    reader_current_page: usize,
    reader_font_size: f32,
}

/// Creates a default instance of `EbookApp` by loading configuration from disk.
///
/// This implementation performs the following steps:
/// 1. Determines the configuration directory using platform-specific paths via `ProjectDirs`
/// 2. Attempts to read and parse `config.json` from the configuration directory
/// 3. Loads previously added books and library path from the config if available
/// 4. If no books are loaded from config:
///    - Loads books from the configured library path on disk if set
///    - Falls back to sample books if no library path is configured
/// 5. Returns a new `EbookApp` instance with the loaded books and library path
///
/// # Behavior
/// - Falls back to current directory (`.`) if platform-specific config directory cannot be determined
/// - Silently ignores missing or invalid config files
/// - Prioritizes explicit configuration over disk scanning
/// - Provides sample books as a last resort to ensure the app is always usable
impl Default for EbookApp {
    fn default() -> Self {
        let config_dir = ProjectDirs::from("", "", "BookLauncher")
            .map(|proj| proj.config_dir().to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));

        let config_path = config_dir.join("config.json");

        let mut books = Vec::new();
        let mut library_path: Option<PathBuf> = None;
        let mut reading_book = None;
        let mut reading_content = String::new();
        let mut reading_error = None;
        let mut reader_current_page = 0;
        let mut reader_font_size = 18.0;

        if let Ok(txt) = std::fs::read_to_string(&config_path) {
            if let Ok(cfg) = serde_json::from_str::<AppConfig>(&txt) {
                books = cfg.added_books;
                library_path = cfg.library_path;
                reading_book = cfg.reading_book;
                reading_content = cfg.reading_content;
                reading_error = cfg.reading_error;
                reader_current_page = cfg.reader_current_page;
                if cfg.reader_font_size.is_finite() && cfg.reader_font_size > 0.0 {
                    reader_font_size = cfg.reader_font_size;
                }
            }
        }

        if books.is_empty() {
            if let Some(dir) = &library_path {
                if let Ok(from_disk) = Book::from_dir(dir) {
                    books = from_disk;
                }
            } else {
                books = Book::sample_books();
            }
        }

        Self {
            books,
            library_path,
            reading_book,
            reading_content,
            reading_error,
            reader_current_page,
            reader_font_size,
            reader_search: String::new(),
            show_library: true,
        }
    }
}

impl EbookApp {
    fn save_config(&self) {
        let config = AppConfig {
            library_path: self.library_path.clone(),
            added_books: self.books.clone(),
            reading_book: self.reading_book.clone(),
            reading_content: self.reading_content.clone(),
            reading_error: self.reading_error.clone(),
            reader_current_page: self.reader_current_page,
            reader_font_size: self.reader_font_size,
        };

        if let Some(proj) = ProjectDirs::from("", "", "BookLauncher") {
            let config_dir = proj.config_dir();
            let _ = std::fs::create_dir_all(config_dir);
            let config_path = config_dir.join("config.json");

            if let Ok(json) = serde_json::to_string_pretty(&config) {
                let _ = std::fs::write(&config_path, json);
            }
        }
    }

    fn supports_in_app_reading(path: &std::path::Path) -> bool {
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| matches!(ext.to_lowercase().as_str(), "txt" | "md" | "epub"))
            .unwrap_or(false)
    }

    fn load_readable_content(path: &Path) -> Result<String, String> {
        match path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_lowercase())
            .as_deref()
        {
            Some("txt") | Some("md") => std::fs::read_to_string(path)
                .map_err(|err| format!("Couldn't read file '{}': {err}", path.display())),
            Some("epub") => Self::read_epub(path),
            _ => Err("Unsupported in-app reader format.".to_string()),
        }
    }

    fn read_epub(path: &Path) -> Result<String, String> {
        let mut doc = EpubDoc::new(path)
            .map_err(|err| format!("Couldn't open EPUB '{}': {err}", path.display()))?;
        let title = doc.get_title();
        let chapter_count = doc.get_num_chapters();
        let mut content = String::new();

        if let Some(title) = title {
            content.push_str(&title);
            content.push_str("\n\n");
        }

        for chapter_index in 0..chapter_count {
            if !doc.set_current_chapter(chapter_index) {
                continue;
            }

            let Some((chapter, mime)) = doc.get_current_str() else {
                continue;
            };

            if !mime.contains("html") && !mime.contains("xml") && !mime.starts_with("text/") {
                continue;
            }

            let chapter_text = Self::html_to_text(&chapter);
            if chapter_text.trim().is_empty() {
                continue;
            }

            if !content.trim().is_empty() {
                content.push_str("\n\n");
            }
            content.push_str(chapter_text.trim());
            content.push('\n');
        }

        if content.trim().is_empty() {
            Err(format!(
                "The EPUB '{}' opened, but no readable chapter text was found.",
                path.display()
            ))
        } else {
            Ok(content)
        }
    }

    fn html_to_text(html: &str) -> String {
        let mut text = String::new();
        let mut tag_buffer = String::new();
        let mut entity = String::new();
        let mut in_ignored_tag: Option<String> = None;
        let mut reading_entity = false;
        let mut inside_tag = false;
        let mut previous_was_space = false;

        fn push_space(text: &mut String, previous_was_space: &mut bool) {
            if !*previous_was_space && !text.is_empty() {
                text.push(' ');
                *previous_was_space = true;
            }
        }

        fn push_newline(text: &mut String, previous_was_space: &mut bool) {
            if !text.ends_with('\n') && !text.is_empty() {
                text.push('\n');
            }
            *previous_was_space = true;
        }

        fn normalize_tag_name(tag: &str) -> String {
            let trimmed = tag.trim();
            let trimmed = trimmed
                .trim_start_matches('/')
                .trim_end_matches('/')
                .trim_start_matches('!')
                .trim_start_matches('?');

            trimmed
                .split_whitespace()
                .next()
                .unwrap_or_default()
                .to_ascii_lowercase()
        }

        for ch in html.chars() {
            if inside_tag {
                tag_buffer.push(ch);

                if ch == '>' {
                    inside_tag = false;
                    let current_tag = normalize_tag_name(&tag_buffer[..tag_buffer.len() - 1]);
                    let is_closing_tag = tag_buffer.trim_start().starts_with("</");

                    match current_tag.as_str() {
                        "style" | "script" | "head" => {
                            if is_closing_tag {
                                in_ignored_tag = None;
                            } else {
                                in_ignored_tag = Some(current_tag.clone());
                            }
                        }
                        "br" | "p" | "div" | "li" | "section" | "article" | "tr"
                        | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                            push_newline(&mut text, &mut previous_was_space);
                        }
                        _ => {}
                    }

                    tag_buffer.clear();
                }
                continue;
            }

            if in_ignored_tag.is_some() {
                if ch == '<' {
                    inside_tag = true;
                    tag_buffer.clear();
                }
                continue;
            }

            if reading_entity {
                if ch == ';' {
                    let decoded = match entity.as_str() {
                        "amp" => Some('&'),
                        "lt" => Some('<'),
                        "gt" => Some('>'),
                        "quot" => Some('"'),
                        "apos" => Some('\''),
                        "nbsp" => Some(' '),
                        "#39" => Some('\''),
                        "#34" => Some('"'),
                        "#38" => Some('&'),
                        "#60" => Some('<'),
                        "#62" => Some('>'),
                        _ => None,
                    };

                    if let Some(decoded) = decoded {
                        text.push(decoded);
                        previous_was_space = decoded.is_whitespace();
                    }

                    entity.clear();
                    reading_entity = false;
                } else if entity.len() < 12 {
                    entity.push(ch);
                } else {
                    entity.clear();
                    reading_entity = false;
                }
                continue;
            }

            match ch {
                '<' => {
                    inside_tag = true;
                    tag_buffer.clear();
                }
                '&' => {
                    reading_entity = true;
                    entity.clear();
                }
                _ if ch.is_whitespace() => {
                    push_space(&mut text, &mut previous_was_space);
                }
                _ => {
                    text.push(ch);
                    previous_was_space = false;
                }
            }
        }

        text.lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn start_reading(&mut self, book: &Book) {
        self.reading_book = Some(book.clone());
        self.reading_content.clear();
        self.reading_error = None;
        self.reader_current_page = 0;
        self.show_library = false;

        let Some(path) = &book.path else {
            self.reading_error = Some("This book does not have a local file path.".to_string());
            self.save_config();
            return;
        };

        if !Self::supports_in_app_reading(path) {
            self.reading_error = Some(format!(
                "{} files are in your library, but the in-app reader currently supports .epub, .txt, and .md content.",
                path.extension()
                    .and_then(|ext| ext.to_str())
                    .unwrap_or("This")
                    .to_uppercase()
            ));
            self.save_config();
            return;
        }

        match Self::load_readable_content(path) {
            Ok(content) => self.reading_content = content,
            Err(err) => self.reading_error = Some(err),
        }
        self.save_config();
    }

    fn close_reader(&mut self) {
        self.show_library = true;
    }

    fn lines_per_page(&self, available_height: f32) -> usize {
        let row_height = (self.reader_font_size + 8.0).max(16.0);
        let reserved_height = 140.0;
        let usable_height = (available_height - reserved_height).max(row_height * 3.0);
        (usable_height / row_height).floor().max(1.0) as usize
    }

    fn matching_lines(&self) -> Vec<(usize, &str)> {
        let query = self.reader_search.trim().to_lowercase();

        if query.is_empty() {
            return self
                .reading_content
                .lines()
                .enumerate()
                .map(|(index, line)| (index + 1, line))
                .collect();
        }

        self.reading_content
            .lines()
            .enumerate()
            .filter(|(_, line)| line.to_lowercase().contains(&query))
            .map(|(index, line)| (index + 1, line))
            .collect()
    }

    fn current_page_lines(&self, lines_per_page: usize) -> (Vec<(usize, String)>, usize) {
        let matching_lines = self.matching_lines();
        let page_count = Self::page_count(matching_lines.len(), lines_per_page);
        let current_page = self.reader_current_page.min(page_count.saturating_sub(1));
        let page_start = current_page.saturating_mul(lines_per_page);
        let page_end = (page_start + lines_per_page).min(matching_lines.len());

        let visible_lines = matching_lines[page_start..page_end]
            .iter()
            .map(|(line_number, line)| (*line_number, (*line).to_owned()))
            .collect();

        (visible_lines, matching_lines.len())
    }

    fn page_count(total_lines: usize, lines_per_page: usize) -> usize {
        total_lines.max(1).div_ceil(lines_per_page.max(1))
    }

    fn clamp_reader_page(&mut self, total_lines: usize, lines_per_page: usize) {
        let last_page = Self::page_count(total_lines, lines_per_page).saturating_sub(1);
        self.reader_current_page = self.reader_current_page.min(last_page);
    }

    fn show_library_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("📚 My E-Book Library");
        ui.separator();

        ui.horizontal(|ui| {
            if ui.button("Choose Library Folder").clicked() {
                if let Some(folder) = FileDialog::new().pick_folder() {
                    self.library_path = Some(folder.clone());

                    if let Ok(new_books) = Book::from_dir(&folder) {
                        self.books = new_books;
                    }

                    self.save_config();
                }
            }

            if ui.button("Select File").clicked() {
                if let Some(path) = FileDialog::new()
                    .add_filter("Ebook", &["epub", "mobi", "azw3", "pdf", "txt", "md"])
                    .pick_file()
                {
                    if let Some(book) = Book::from_filename(&path) {
                        self.books.push(book);
                        self.save_config();
                    }
                }
            }
        });

        ui.separator();

        egui::ScrollArea::vertical()
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                for book in self.books.clone() {
                    ui.group(|ui| {
                        ui.label(format!("📖 {}", book.title));
                        ui.label(format!("👤 {}", book.author));

                        ui.horizontal(|ui| {
                            if ui.button("Read in App").clicked() {
                                self.start_reading(&book);
                            }

                            if let Some(path) = &book.path {
                                if ui.button("Open Externally").clicked() {
                                    let _ = open::that(path);
                                }
                            }
                        });
                    });
                    ui.add_space(8.0);
                }
            });
    }

    fn show_reader_ui(&mut self, ui: &mut egui::Ui, book: &Book) {
        let lines_per_page = self.lines_per_page(ui.available_height());

        ui.horizontal(|ui| {
            if ui.button("Back to Library").clicked() {
                self.close_reader();
            }

            ui.separator();
            ui.label(format!("📖 {}", book.title));
            ui.label(format!("by {}", book.author));
        });

        ui.add_space(6.0);

        ui.horizontal(|ui| {
            ui.label("Text size");
            let changed = ui
                .add(egui::Slider::new(&mut self.reader_font_size, 12.0..=32.0).suffix(" px"))
                .changed();

            if changed {
                self.clamp_reader_page(self.reading_content.lines().count(), lines_per_page);
                self.save_config();
            }

            ui.separator();
            ui.label("Search");
            if ui.text_edit_singleline(&mut self.reader_search).changed() {
                self.reader_current_page = 0;
            }
        });

        ui.separator();

        if let Some(err) = &self.reading_error {
            ui.colored_label(egui::Color32::from_rgb(220, 80, 80), err);

            if let Some(path) = &book.path {
                if ui.button("Open This Book Externally").clicked() {
                    let _ = open::that(path);
                }
            }

            return;
        }

        let total_matching_lines = self.matching_lines().len();
        self.clamp_reader_page(total_matching_lines, lines_per_page);
        let page_count = Self::page_count(total_matching_lines, lines_per_page);
        let can_go_back = self.reader_current_page > 0;
        let can_go_forward = self.reader_current_page + 1 < page_count;

        ui.horizontal(|ui| {
            if ui
                .add_enabled(can_go_back, egui::Button::new("Previous Page"))
                .clicked()
            {
                self.reader_current_page = self.reader_current_page.saturating_sub(1);
                self.save_config();
            }

            ui.label(format!(
                "Page {} of {}",
                self.reader_current_page + 1,
                page_count
            ));

            if ui
                .add_enabled(can_go_forward, egui::Button::new("Next Page"))
                .clicked()
            {
                self.reader_current_page += 1;
                self.save_config();
            }
        });

        if !self.reader_search.trim().is_empty() {
            ui.label(format!("{} matching lines", total_matching_lines));
        } else {
            ui.label(format!("{} lines per page", lines_per_page));
        }

        if ui.input(|input| input.key_pressed(egui::Key::ArrowLeft)) && can_go_back {
            self.reader_current_page = self.reader_current_page.saturating_sub(1);
            self.save_config();
        }

        if ui.input(|input| input.key_pressed(egui::Key::ArrowRight)) && can_go_forward {
            self.reader_current_page += 1;
            self.save_config();
        }

        let (visible_lines, _) = self.current_page_lines(lines_per_page);

        egui::ScrollArea::vertical()
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                for (line_number, line) in &visible_lines {
                    ui.horizontal_top(|ui| {
                        ui.add_sized(
                            [48.0, self.reader_font_size + 6.0],
                            egui::Label::new(
                                egui::RichText::new(line_number.to_string())
                                    .size(12.0)
                                    .color(egui::Color32::GRAY),
                                ),
                        );
                        ui.label(egui::RichText::new(line).size(self.reader_font_size));
                    });
                }
            });
    }
}

impl eframe::App for EbookApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            if self.show_library {
                self.show_library_ui(ui);
            } else if let Some(book) = self.reading_book.clone() {
                self.show_reader_ui(ui, &book);
            } else {
                self.show_library = true;
                self.show_library_ui(ui);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::EbookApp;

    #[test]
    fn html_to_text_removes_markup_and_decodes_common_entities() {
        let html = r#"
            <html>
                <body>
                    <h1>Chapter &amp; One</h1>
                    <p>This is <em>EPUB</em> text&nbsp;inside XHTML.</p>
                </body>
            </html>
        "#;

        let text = EbookApp::html_to_text(html);

        assert!(text.contains("Chapter & One"));
        assert!(text.contains("This is EPUB text inside XHTML."));
        assert!(!text.contains("<em>"));
    }

    #[test]
    fn html_to_text_ignores_style_content() {
        let html = r#"
            <html>
                <head>
                    <style>@page { margin-bottom: 5pt; }</style>
                </head>
                <body>
                    <p>Hello world.</p>
                </body>
            </html>
        "#;

        let text = EbookApp::html_to_text(html);

        assert!(text.contains("Hello world."));
        assert!(!text.contains("@page"));
        assert!(!text.contains("margin-bottom"));
    }

    #[test]
    fn page_count_rounds_up_and_never_returns_zero() {
        assert_eq!(EbookApp::page_count(0, 20), 1);
        assert_eq!(EbookApp::page_count(20, 20), 1);
        assert_eq!(EbookApp::page_count(21, 20), 2);
    }
}
