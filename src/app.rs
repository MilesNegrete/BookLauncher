use crate::book::Book;
use directories::ProjectDirs;
use eframe::egui;
use epub::doc::EpubDoc;
use image::ImageFormat;
use rfd::FileDialog;
use std::collections::{hash_map::DefaultHasher, HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct EbookApp {
    books: Vec<Book>,
    library_path: Option<PathBuf>,
    library_cleared: bool,
    reading_book: Option<Book>,
    reading_content: Arc<String>,
    reading_error: Option<String>,
    reading_loading: bool,
    content_revision: u64,
    load_generation: u64,
    load_result_sender: mpsc::Sender<BookLoadResult>,
    load_result_receiver: mpsc::Receiver<BookLoadResult>,
    layout_sender: mpsc::Sender<LayoutRequest>,
    layout_receiver: mpsc::Receiver<LayoutResult>,
    layout_lines: Vec<(usize, ReaderLine)>,
    layout_pending: Option<LayoutKey>,
    layout_applied: Option<LayoutKey>,
    pdf_document: Option<PdfDocument>,
    pdf_page_sender: mpsc::Sender<PdfPageRequest>,
    pdf_page_receiver: mpsc::Receiver<PdfPageResult>,
    pdf_page_pending: Option<usize>,
    pdf_page_texture: Option<(usize, egui::TextureHandle)>,
    cover_sender: mpsc::Sender<CoverRequest>,
    cover_receiver: mpsc::Receiver<CoverResult>,
    cover_pending: HashSet<String>,
    cover_textures: HashMap<String, Option<egui::TextureHandle>>,
    reader_current_page: usize,
    reader_font_size: f32,
    reader_search: String,
    show_library: bool,
    confirm_clear_library: bool,
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
#[serde(default)]
struct AppConfig {
    library_path: Option<PathBuf>,
    added_books: Vec<Book>,
    library_cleared: bool,
    reading_book: Option<Book>,
    reading_content: String,
    reading_error: Option<String>,
    reader_current_page: usize,
    reader_font_size: f32,
}

struct BookLoadResult {
    generation: u64,
    content: Result<LoadedBook, String>,
}

enum LoadedBook {
    Text(String),
    Pdf(PdfDocument),
}

struct PdfDocument {
    path: PathBuf,
    page_count: usize,
}

struct PdfPageRequest {
    generation: u64,
    path: PathBuf,
    page: usize,
    repaint_ctx: egui::Context,
}

struct PdfPageResult {
    generation: u64,
    page: usize,
    image: Result<egui::ColorImage, String>,
}

struct CoverRequest {
    key: String,
    path: PathBuf,
    repaint_ctx: egui::Context,
}

struct CoverResult {
    key: String,
    image: Result<egui::ColorImage, String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct LayoutKey {
    content_revision: u64,
    chars_per_line: usize,
    query: String,
}

struct LayoutRequest {
    key: LayoutKey,
    content: Arc<String>,
    repaint_ctx: egui::Context,
}

struct LayoutResult {
    key: LayoutKey,
    lines: Vec<(usize, ReaderLine)>,
}

/// Creates a default instance of `EbookApp` by loading configuration from disk.
///
/// This implementation performs the following steps:
/// 1. Determines the configuration directory using platform-specific paths via `ProjectDirs`
/// 2. Attempts to read and parse `config.json` from the configuration directory
/// 3. Loads previously added books and library path from the config if available
/// 4. If no books are loaded from config and the library was not explicitly cleared:
///    - Loads books from the configured library path on disk if set
///    - Falls back to sample books if no library path is configured
/// 5. Returns a new `EbookApp` instance with the loaded books and library path
///
/// # Behavior
/// - Falls back to current directory (`.`) if platform-specific config directory cannot be determined
/// - Silently ignores missing or invalid config files
/// - Prioritizes explicit configuration over disk scanning
/// - Provides sample books as a last resort unless the user explicitly cleared the library
impl Default for EbookApp {
    fn default() -> Self {
        let config_dir = ProjectDirs::from("", "", "BookLauncher")
            .map(|proj| proj.config_dir().to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));

        let config_path = config_dir.join("config.json");

        let mut books = Vec::new();
        let mut library_path: Option<PathBuf> = None;
        let mut library_cleared = false;
        let mut reading_book = None;
        let mut reading_content = String::new();
        let mut reading_error = None;
        let mut reader_current_page = 0;
        let mut reader_font_size = 18.0;

        if let Ok(txt) = std::fs::read_to_string(&config_path) {
            if let Ok(cfg) = serde_json::from_str::<AppConfig>(&txt) {
                books = cfg.added_books;
                library_path = cfg.library_path;
                library_cleared = cfg.library_cleared;
                reading_book = cfg.reading_book;
                reading_content = cfg.reading_content;
                reading_error = cfg.reading_error;
                reader_current_page = cfg.reader_current_page;
                if cfg.reader_font_size.is_finite() && cfg.reader_font_size > 0.0 {
                    reader_font_size = cfg.reader_font_size;
                }
            }
        }

        if books.is_empty() && !library_cleared {
            if let Some(dir) = &library_path {
                if let Ok(from_disk) = Book::from_dir(dir) {
                    books = from_disk;
                }
            } else {
                books = Book::sample_books();
            }
        }

        let content_revision = u64::from(!reading_content.is_empty());
        let (load_result_sender, load_result_receiver) = mpsc::channel();
        let (layout_sender, layout_receiver) = Self::spawn_layout_worker();
        let (pdf_page_sender, pdf_page_receiver) = Self::spawn_pdf_page_worker();
        let (cover_sender, cover_receiver) = Self::spawn_cover_worker();

        Self {
            books,
            library_path,
            library_cleared,
            reading_book,
            reading_content: Arc::new(reading_content),
            reading_error,
            reading_loading: false,
            content_revision,
            load_generation: 0,
            load_result_sender,
            load_result_receiver,
            layout_sender,
            layout_receiver,
            layout_lines: Vec::new(),
            layout_pending: None,
            layout_applied: None,
            pdf_document: None,
            pdf_page_sender,
            pdf_page_receiver,
            pdf_page_pending: None,
            pdf_page_texture: None,
            cover_sender,
            cover_receiver,
            cover_pending: HashSet::new(),
            cover_textures: HashMap::new(),
            reader_current_page,
            reader_font_size,
            reader_search: String::new(),
            show_library: true,
            confirm_clear_library: false,
        }
    }
}

impl EbookApp {
    const STYLE_MARKER: char = '\u{1f}';

    fn spawn_layout_worker() -> (mpsc::Sender<LayoutRequest>, mpsc::Receiver<LayoutResult>) {
        let (request_sender, request_receiver) = mpsc::channel::<LayoutRequest>();
        let (result_sender, result_receiver) = mpsc::channel();

        thread::spawn(move || {
            while let Ok(mut request) = request_receiver.recv() {
                while let Ok(newer_request) = request_receiver.try_recv() {
                    request = newer_request;
                }

                let lines = Self::matching_lines_for(
                    &request.content,
                    request.key.chars_per_line,
                    &request.key.query,
                );
                if result_sender
                    .send(LayoutResult {
                        key: request.key,
                        lines,
                    })
                    .is_err()
                {
                    break;
                }
                request.repaint_ctx.request_repaint();
            }
        });

        (request_sender, result_receiver)
    }

    fn spawn_pdf_page_worker() -> (mpsc::Sender<PdfPageRequest>, mpsc::Receiver<PdfPageResult>) {
        let (request_sender, request_receiver) = mpsc::channel::<PdfPageRequest>();
        let (result_sender, result_receiver) = mpsc::channel();

        thread::spawn(move || {
            while let Ok(mut request) = request_receiver.recv() {
                while let Ok(newer_request) = request_receiver.try_recv() {
                    request = newer_request;
                }

                let image = Self::render_pdf_page(&request.path, request.page);
                if result_sender
                    .send(PdfPageResult {
                        generation: request.generation,
                        page: request.page,
                        image,
                    })
                    .is_err()
                {
                    break;
                }
                request.repaint_ctx.request_repaint();
            }
        });

        (request_sender, result_receiver)
    }

    fn spawn_cover_worker() -> (mpsc::Sender<CoverRequest>, mpsc::Receiver<CoverResult>) {
        let (request_sender, request_receiver) = mpsc::channel::<CoverRequest>();
        let (result_sender, result_receiver) = mpsc::channel();

        thread::spawn(move || {
            while let Ok(request) = request_receiver.recv() {
                let image = Self::load_or_extract_cover(&request.path, &request.key);
                if result_sender
                    .send(CoverResult {
                        key: request.key,
                        image,
                    })
                    .is_err()
                {
                    break;
                }
                request.repaint_ctx.request_repaint();
            }
        });

        (request_sender, result_receiver)
    }

    fn cover_cache_dir() -> PathBuf {
        ProjectDirs::from("", "", "BookLauncher")
            .map(|proj| proj.cache_dir().join("covers"))
            .unwrap_or_else(|| std::env::temp_dir().join("booklauncher-covers"))
    }

    fn cover_key(path: &Path) -> Option<String> {
        let metadata = std::fs::metadata(path).ok()?;
        let modified = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        let mut hasher = DefaultHasher::new();
        path.hash(&mut hasher);
        metadata.len().hash(&mut hasher);
        modified.hash(&mut hasher);
        Some(format!("{:016x}", hasher.finish()))
    }

    fn load_or_extract_cover(path: &Path, key: &str) -> Result<egui::ColorImage, String> {
        let cache_dir = Self::cover_cache_dir();
        let cache_path = cache_dir.join(format!("{key}.png"));
        if let Ok(bytes) = std::fs::read(&cache_path) {
            if let Ok(image) = Self::decode_cover_image(&bytes) {
                return Ok(image);
            }
            let _ = std::fs::remove_file(&cache_path);
        }

        let bytes = Self::extract_cover(path)?;
        let image = image::load_from_memory(&bytes)
            .map_err(|err| format!("Couldn't decode cover for '{}': {err}", path.display()))?
            .thumbnail(120, 180)
            .to_rgba8();
        let _ = std::fs::create_dir_all(&cache_dir);
        let _ = image.save_with_format(&cache_path, ImageFormat::Png);
        Self::color_image_from_rgba(image)
    }

    fn decode_cover_image(bytes: &[u8]) -> Result<egui::ColorImage, String> {
        let image = image::load_from_memory(bytes)
            .map_err(|err| format!("Couldn't decode cached cover: {err}"))?
            .to_rgba8();
        Self::color_image_from_rgba(image)
    }

    fn color_image_from_rgba(image: image::RgbaImage) -> Result<egui::ColorImage, String> {
        let width = usize::try_from(image.width())
            .map_err(|_| "Cover image width is too large.".to_string())?;
        let height = usize::try_from(image.height())
            .map_err(|_| "Cover image height is too large.".to_string())?;
        Ok(egui::ColorImage::from_rgba_unmultiplied(
            [width, height],
            image.as_raw(),
        ))
    }

    fn extract_cover(path: &Path) -> Result<Vec<u8>, String> {
        match path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("epub") => {
                let mut doc = EpubDoc::new(path)
                    .map_err(|err| format!("Couldn't open EPUB '{}': {err}", path.display()))?;
                if let Some((cover, _)) = doc.get_cover() {
                    return Ok(cover);
                }

                let cover_id = doc.resources.iter().find_map(|(id, resource)| {
                    let candidate = format!("{} {}", id, resource.path.display()).to_lowercase();
                    (resource.mime.starts_with("image/") && candidate.contains("cover"))
                        .then(|| id.clone())
                });
                cover_id
                    .and_then(|id| doc.get_resource(&id))
                    .map(|(cover, _)| cover)
                    .ok_or_else(|| format!("No embedded cover found in '{}'.", path.display()))
            }
            Some("mobi") | Some("azw3") => Self::extract_calibre_cover(path),
            Some("pdf") => Self::extract_pdf_cover(path),
            _ => Err(format!("No cover extractor for '{}'.", path.display())),
        }
    }

    fn extract_calibre_cover(path: &Path) -> Result<Vec<u8>, String> {
        let output_path = Self::temporary_cover_path("jpg");
        let output = Command::new("ebook-meta")
            .arg(path)
            .arg("--get-cover")
            .arg(&output_path)
            .output()
            .map_err(|err| format!("Couldn't start ebook-meta for '{}': {err}", path.display()))?;
        Self::read_extracted_cover(path, output, &output_path)
    }

    fn extract_pdf_cover(path: &Path) -> Result<Vec<u8>, String> {
        let output_prefix = Self::temporary_cover_path("page");
        let mut output_path = output_prefix.as_os_str().to_os_string();
        output_path.push(".png");
        let output_path = PathBuf::from(output_path);
        let output = Command::new("pdftoppm")
            .arg("-f")
            .arg("1")
            .arg("-l")
            .arg("1")
            .arg("-singlefile")
            .arg("-scale-to")
            .arg("360")
            .arg("-png")
            .arg(path)
            .arg(&output_prefix)
            .output()
            .map_err(|err| format!("Couldn't start pdftoppm for '{}': {err}", path.display()))?;
        Self::read_extracted_cover(path, output, &output_path)
    }

    fn temporary_cover_path(extension: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "booklauncher-cover-{}-{unique}.{extension}",
            std::process::id()
        ))
    }

    fn read_extracted_cover(
        source_path: &Path,
        output: std::process::Output,
        output_path: &Path,
    ) -> Result<Vec<u8>, String> {
        if !output.status.success() {
            let _ = std::fs::remove_file(output_path);
            return Err(format!(
                "Couldn't extract cover from '{}': {}",
                source_path.display(),
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        let bytes = std::fs::read(output_path)
            .map_err(|err| format!("Couldn't read extracted cover: {err}"));
        let _ = std::fs::remove_file(output_path);
        bytes
    }

    fn save_config(&self) {
        let config = AppConfig {
            library_path: self.library_path.clone(),
            added_books: self.books.clone(),
            library_cleared: self.library_cleared,
            reading_book: self.reading_book.clone(),
            // Book content is loaded asynchronously when opened. Avoid serializing an entire
            // ebook whenever the user flips a page.
            reading_content: String::new(),
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
            .map(|ext| {
                matches!(
                    ext.to_lowercase().as_str(),
                    "txt" | "md" | "epub" | "pdf" | "mobi" | "azw3"
                )
            })
            .unwrap_or(false)
    }

    fn load_readable_content(path: &Path) -> Result<LoadedBook, String> {
        match path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_lowercase())
            .as_deref()
        {
            Some("txt") | Some("md") => std::fs::read_to_string(path)
                .map(LoadedBook::Text)
                .map_err(|err| format!("Couldn't read file '{}': {err}", path.display())),
            Some("epub") => Self::read_epub(path).map(LoadedBook::Text),
            Some("pdf") => Self::read_pdf(path),
            Some("mobi") | Some("azw3") => Self::read_calibre_ebook(path).map(LoadedBook::Text),
            _ => Err("Unsupported in-app reader format.".to_string()),
        }
    }

    fn read_pdf(path: &Path) -> Result<LoadedBook, String> {
        let output = Command::new("pdfinfo")
            .arg(path)
            .output()
            .map_err(|err| {
                format!(
                    "Couldn't start pdfinfo for '{}': {err}. Install poppler-utils to read PDFs in the app.",
                    path.display()
                )
            })?;

        if !output.status.success() {
            return Err(format!(
                "Couldn't inspect PDF '{}': {}",
                path.display(),
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }

        let info = String::from_utf8_lossy(&output.stdout);
        let page_count = info
            .lines()
            .find_map(|line| line.strip_prefix("Pages:"))
            .and_then(|pages| pages.trim().parse::<usize>().ok())
            .filter(|pages| *pages > 0)
            .ok_or_else(|| {
                format!(
                    "Couldn't determine the page count for PDF '{}'.",
                    path.display()
                )
            })?;

        Ok(LoadedBook::Pdf(PdfDocument {
            path: path.to_path_buf(),
            page_count,
        }))
    }

    fn render_pdf_page(path: &Path, page: usize) -> Result<egui::ColorImage, String> {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let output_prefix =
            std::env::temp_dir().join(format!("booklauncher-pdf-{}-{unique}", std::process::id()));
        let output_path = output_prefix.with_extension("ppm");

        let output = Command::new("pdftoppm")
            .arg("-f")
            .arg((page + 1).to_string())
            .arg("-l")
            .arg((page + 1).to_string())
            .arg("-singlefile")
            .arg("-scale-to")
            .arg("1800")
            .arg(path)
            .arg(&output_prefix)
            .output()
            .map_err(|err| {
                format!(
                    "Couldn't start pdftoppm for '{}': {err}. Install poppler-utils to display PDFs in the app.",
                    path.display()
                )
            })?;

        if !output.status.success() {
            let _ = std::fs::remove_file(&output_path);
            return Err(format!(
                "Couldn't render PDF page {} from '{}': {}",
                page + 1,
                path.display(),
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }

        let ppm = std::fs::read(&output_path)
            .map_err(|err| format!("Couldn't read rendered PDF page: {err}"));
        let _ = std::fs::remove_file(output_path);
        Self::decode_ppm(&ppm?)
    }

    fn decode_ppm(ppm: &[u8]) -> Result<egui::ColorImage, String> {
        fn next_token<'a>(ppm: &'a [u8], position: &mut usize) -> Option<&'a [u8]> {
            loop {
                while ppm.get(*position).is_some_and(u8::is_ascii_whitespace) {
                    *position += 1;
                }
                if ppm.get(*position) != Some(&b'#') {
                    break;
                }
                while ppm.get(*position).is_some_and(|byte| *byte != b'\n') {
                    *position += 1;
                }
            }

            let start = *position;
            while ppm
                .get(*position)
                .is_some_and(|byte| !byte.is_ascii_whitespace())
            {
                *position += 1;
            }
            (start < *position).then(|| &ppm[start..*position])
        }

        fn parse_usize(token: Option<&[u8]>, field: &str) -> Result<usize, String> {
            std::str::from_utf8(token.ok_or_else(|| format!("Missing PPM {field}."))?)
                .map_err(|err| format!("Invalid PPM {field}: {err}"))?
                .parse::<usize>()
                .map_err(|err| format!("Invalid PPM {field}: {err}"))
        }

        let mut position = 0;
        if next_token(ppm, &mut position) != Some(b"P6") {
            return Err("Rendered PDF page is not a binary PPM image.".to_string());
        }
        let width = parse_usize(next_token(ppm, &mut position), "width")?;
        let height = parse_usize(next_token(ppm, &mut position), "height")?;
        if parse_usize(next_token(ppm, &mut position), "maximum color value")? != 255 {
            return Err("Rendered PDF page uses an unsupported PPM color depth.".to_string());
        }
        match ppm.get(position..position + 2) {
            Some(b"\r\n") => position += 2,
            _ if ppm.get(position).is_some_and(u8::is_ascii_whitespace) => position += 1,
            _ => return Err("Rendered PDF page is missing its PPM data separator.".to_string()),
        }

        let expected_bytes = width
            .checked_mul(height)
            .and_then(|pixels| pixels.checked_mul(3))
            .ok_or_else(|| "Rendered PDF page dimensions are too large.".to_string())?;
        let rgb = ppm
            .get(position..position + expected_bytes)
            .ok_or_else(|| "Rendered PDF page data is incomplete.".to_string())?;

        Ok(egui::ColorImage::from_rgb([width, height], rgb))
    }

    fn read_calibre_ebook(path: &Path) -> Result<String, String> {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let output_path =
            std::env::temp_dir().join(format!("booklauncher-{}-{unique}.txt", std::process::id()));

        let output = Command::new("ebook-convert")
            .arg(path)
            .arg(&output_path)
            .output()
            .map_err(|err| {
                format!(
                    "Couldn't start ebook-convert for '{}': {err}. Install Calibre to read MOBI and AZW3 files in the app.",
                    path.display()
                )
            })?;

        if !output.status.success() {
            let _ = std::fs::remove_file(&output_path);
            return Err(format!(
                "Couldn't extract text from ebook '{}': {}",
                path.display(),
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }

        let content = std::fs::read_to_string(&output_path)
            .map_err(|err| format!("Couldn't read converted ebook text: {err}"));
        let _ = std::fs::remove_file(output_path);

        let content = content?;
        if content.trim().is_empty() {
            Err(format!(
                "The ebook '{}' opened, but no readable text was found.",
                path.display()
            ))
        } else {
            Ok(content)
        }
    }

    fn read_epub(path: &Path) -> Result<String, String> {
        let mut doc = EpubDoc::new(path)
            .map_err(|err| format!("Couldn't open EPUB '{}': {err}", path.display()))?;
        let title = doc.get_title();
        let chapter_count = doc.get_num_chapters();
        let mut content = String::new();

        if let Some(title) = title {
            content.push_str(&Self::styled_text(ReaderTextStyle::Title, &title));
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
        let mut ignored_tags = Vec::new();
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
                    let is_closing_tag = tag_buffer.trim_start().starts_with('/');

                    match current_tag.as_str() {
                        "style" | "script" | "head" => {
                            if is_closing_tag {
                                if ignored_tags.last() == Some(&current_tag) {
                                    ignored_tags.pop();
                                }
                            } else {
                                ignored_tags.push(current_tag.clone());
                            }
                        }
                        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "header" if !is_closing_tag => {
                            push_newline(&mut text, &mut previous_was_space);
                            text.push_str(&Self::style_prefix(match current_tag.as_str() {
                                "h1" => ReaderTextStyle::Heading1,
                                "h2" | "header" => ReaderTextStyle::Heading2,
                                _ => ReaderTextStyle::Heading3,
                            }));
                            previous_was_space = false;
                        }
                        "br" | "p" | "div" | "li" | "section" | "article" | "tr" | "h1" | "h2"
                        | "h3" | "h4" | "h5" | "h6" | "header" => {
                            push_newline(&mut text, &mut previous_was_space);
                        }
                        _ => {}
                    }

                    tag_buffer.clear();
                }
                continue;
            }

            if !ignored_tags.is_empty() {
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

    fn start_reading(&mut self, book: &Book, repaint_ctx: egui::Context) {
        self.reading_book = Some(book.clone());
        self.reading_content = Arc::new(String::new());
        self.reading_error = None;
        self.reading_loading = false;
        self.pdf_document = None;
        self.pdf_page_pending = None;
        self.pdf_page_texture = None;
        self.reader_current_page = 0;
        self.show_library = false;
        self.invalidate_layout();
        self.load_generation = self.load_generation.wrapping_add(1);

        let Some(path) = &book.path else {
            self.reading_error = Some("This book does not have a local file path.".to_string());
            self.save_config();
            return;
        };

        if !Self::supports_in_app_reading(path) {
            self.reading_error = Some(format!(
                "{} files are in your library, but the in-app reader currently supports .epub, .pdf, .mobi, .azw3, .txt, and .md content.",
                path.extension()
                    .and_then(|ext| ext.to_str())
                    .unwrap_or("This")
                    .to_uppercase()
            ));
            self.save_config();
            return;
        }

        let generation = self.load_generation;
        let path = path.clone();
        let result_sender = self.load_result_sender.clone();
        self.reading_loading = true;
        thread::spawn(move || {
            let content = Self::load_readable_content(&path);
            let _ = result_sender.send(BookLoadResult {
                generation,
                content,
            });
            repaint_ctx.request_repaint();
        });
        self.save_config();
    }

    fn close_reader(&mut self) {
        self.show_library = true;
    }

    fn invalidate_layout(&mut self) {
        self.layout_lines.clear();
        self.layout_pending = None;
        self.layout_applied = None;
    }

    fn poll_background_jobs(&mut self, ctx: &egui::Context) {
        while let Ok(result) = self.load_result_receiver.try_recv() {
            if result.generation != self.load_generation {
                continue;
            }

            self.reading_loading = false;
            match result.content {
                Ok(LoadedBook::Text(content)) => {
                    self.reading_content = Arc::new(content);
                    self.pdf_document = None;
                    self.pdf_page_pending = None;
                    self.pdf_page_texture = None;
                    self.content_revision = self.content_revision.wrapping_add(1);
                    self.reading_error = None;
                    self.invalidate_layout();
                }
                Ok(LoadedBook::Pdf(document)) => {
                    self.reading_content = Arc::new(String::new());
                    self.pdf_document = Some(document);
                    self.pdf_page_pending = None;
                    self.pdf_page_texture = None;
                    self.reading_error = None;
                    self.invalidate_layout();
                }
                Err(err) => {
                    self.reading_error = Some(err);
                    self.invalidate_layout();
                }
            }
            self.save_config();
        }

        while let Ok(result) = self.layout_receiver.try_recv() {
            if self.layout_pending.as_ref() == Some(&result.key) {
                self.layout_lines = result.lines;
                self.layout_applied = Some(result.key);
                self.layout_pending = None;
            }
        }

        while let Ok(result) = self.pdf_page_receiver.try_recv() {
            if result.generation != self.load_generation
                || self.pdf_page_pending != Some(result.page)
            {
                continue;
            }

            self.pdf_page_pending = None;
            match result.image {
                Ok(image) => {
                    let texture = ctx.load_texture(
                        format!("pdf-page-{}", result.page),
                        image,
                        egui::TextureOptions::LINEAR,
                    );
                    self.pdf_page_texture = Some((result.page, texture));
                }
                Err(err) => self.reading_error = Some(err),
            }
        }

        while let Ok(result) = self.cover_receiver.try_recv() {
            self.cover_pending.remove(&result.key);
            let texture = result.image.ok().map(|image| {
                ctx.load_texture(
                    format!("book-cover-{}", result.key),
                    image,
                    egui::TextureOptions::LINEAR,
                )
            });
            self.cover_textures.insert(result.key, texture);
        }
    }

    fn request_cover(&mut self, book: &Book, repaint_ctx: egui::Context) -> Option<String> {
        let path = book.path.as_deref()?;
        let key = Self::cover_key(path)?;
        if !self.cover_textures.contains_key(&key)
            && !self.cover_pending.contains(&key)
            && self
                .cover_sender
                .send(CoverRequest {
                    key: key.clone(),
                    path: path.to_path_buf(),
                    repaint_ctx,
                })
                .is_ok()
        {
            self.cover_pending.insert(key.clone());
        }
        Some(key)
    }

    fn show_book_cover(&mut self, ui: &mut egui::Ui, book: &Book) {
        const COVER_SIZE: egui::Vec2 = egui::vec2(80.0, 120.0);
        let key = self.request_cover(book, ui.ctx().clone());
        let texture = key
            .as_ref()
            .and_then(|key| self.cover_textures.get(key))
            .and_then(Option::as_ref);
        let loading = key
            .as_ref()
            .is_some_and(|key| self.cover_pending.contains(key));

        if let Some(texture) = texture {
            let texture_size = texture.size_vec2();
            let scale = (COVER_SIZE.x / texture_size.x)
                .min(COVER_SIZE.y / texture_size.y)
                .min(1.0);
            ui.add_sized(
                COVER_SIZE,
                egui::Image::new((texture.id(), texture_size * scale)),
            );
        } else {
            ui.allocate_ui_with_layout(
                COVER_SIZE,
                egui::Layout::top_down(egui::Align::Center).with_cross_justify(true),
                |ui| {
                    ui.add_space(42.0);
                    ui.label(if loading {
                        "Loading cover..."
                    } else {
                        "No cover"
                    });
                },
            );
        }
    }

    fn request_pdf_page(&mut self, repaint_ctx: egui::Context) {
        let Some(document) = &self.pdf_document else {
            return;
        };
        let page = self
            .reader_current_page
            .min(document.page_count.saturating_sub(1));

        if self.pdf_page_texture.as_ref().map(|(page, _)| *page) == Some(page)
            || self.pdf_page_pending == Some(page)
        {
            return;
        }

        if self
            .pdf_page_sender
            .send(PdfPageRequest {
                generation: self.load_generation,
                path: document.path.clone(),
                page,
                repaint_ctx,
            })
            .is_ok()
        {
            self.pdf_page_pending = Some(page);
        }
    }

    fn style_prefix(style: ReaderTextStyle) -> String {
        format!("{}{}\t", Self::STYLE_MARKER, style.code())
    }

    fn styled_text(style: ReaderTextStyle, text: &str) -> String {
        format!("{}{}", Self::style_prefix(style), text)
    }

    fn parse_styled_text(line: &str) -> (ReaderTextStyle, &str) {
        let Some(rest) = line.strip_prefix(Self::STYLE_MARKER) else {
            return (ReaderTextStyle::Body, line);
        };
        let Some((code, text)) = rest.split_once('\t') else {
            return (ReaderTextStyle::Body, line);
        };

        (ReaderTextStyle::from_code(code), text)
    }

    fn lines_per_page(&self, available_height: f32) -> usize {
        let row_height = (self.reader_font_size + 8.0).max(16.0);
        let reserved_height = 140.0;
        let usable_height = (available_height - reserved_height).max(row_height * 3.0);
        (usable_height / row_height).floor().max(1.0) as usize
    }

    fn chars_per_line(&self, available_width: f32) -> usize {
        let text_width = (available_width - 72.0).max(120.0);
        (text_width / (self.reader_font_size * 0.55).max(1.0))
            .floor()
            .max(12.0) as usize
    }

    fn wrapped_lines(content: &str, chars_per_line: usize) -> Vec<ReaderLine> {
        let mut wrapped = Vec::new();
        let chars_per_line = chars_per_line.max(1);

        for paragraph in content.lines() {
            let paragraph = paragraph.trim();
            if paragraph.is_empty() {
                continue;
            }
            let (style, paragraph) = Self::parse_styled_text(paragraph);

            let mut line = String::new();
            for word in paragraph.split_whitespace() {
                let separator_width = usize::from(!line.is_empty());
                if !line.is_empty()
                    && line.chars().count() + separator_width + word.chars().count()
                        > chars_per_line
                {
                    wrapped.push(ReaderLine::new(std::mem::take(&mut line), style));
                }

                if !line.is_empty() {
                    line.push(' ');
                }
                line.push_str(word);
            }

            if !line.is_empty() {
                wrapped.push(ReaderLine::new(line, style));
            }
        }

        wrapped
    }

    fn matching_lines_for(
        content: &str,
        chars_per_line: usize,
        normalized_query: &str,
    ) -> Vec<(usize, ReaderLine)> {
        let wrapped_lines = Self::wrapped_lines(content, chars_per_line);

        if normalized_query.is_empty() {
            return wrapped_lines
                .into_iter()
                .enumerate()
                .map(|(index, line)| (index + 1, line))
                .collect();
        }

        wrapped_lines
            .into_iter()
            .enumerate()
            .filter(|(_, line)| line.text.to_lowercase().contains(normalized_query))
            .map(|(index, line)| (index + 1, line))
            .collect()
    }

    fn request_layout(&mut self, chars_per_line: usize, repaint_ctx: egui::Context) {
        if self.reading_loading || self.reading_error.is_some() {
            return;
        }

        let key = LayoutKey {
            content_revision: self.content_revision,
            chars_per_line,
            query: self.reader_search.trim().to_lowercase(),
        };

        if self.layout_applied.as_ref() == Some(&key) || self.layout_pending.as_ref() == Some(&key)
        {
            return;
        }

        if self
            .layout_sender
            .send(LayoutRequest {
                key: key.clone(),
                content: Arc::clone(&self.reading_content),
                repaint_ctx,
            })
            .is_ok()
        {
            self.layout_pending = Some(key);
        }
    }

    fn current_page_lines(&self, lines_per_page: usize) -> Vec<(usize, ReaderLine)> {
        let page_count = Self::page_count(self.layout_lines.len(), lines_per_page);
        let current_page = self.reader_current_page.min(page_count.saturating_sub(1));
        let page_start = current_page.saturating_mul(lines_per_page);
        let page_end = (page_start + lines_per_page).min(self.layout_lines.len());

        self.layout_lines[page_start..page_end]
            .iter()
            .map(|(line_number, line)| (*line_number, line.clone()))
            .collect()
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
                    self.library_cleared = false;

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
                        self.library_cleared = false;
                        self.save_config();
                    }
                }
            }

            if ui
                .add_enabled(!self.books.is_empty(), egui::Button::new("Clear Library"))
                .clicked()
            {
                self.confirm_clear_library = true;
            }

            if self.confirm_clear_library && ui.button("Confirm Clear").clicked() {
                self.books.clear();
                self.library_path = None;
                self.library_cleared = true;
                self.reading_book = None;
                self.reading_content = Arc::new(String::new());
                self.reading_error = None;
                self.reading_loading = false;
                self.pdf_document = None;
                self.pdf_page_pending = None;
                self.pdf_page_texture = None;
                self.cover_pending.clear();
                self.cover_textures.clear();
                self.load_generation = self.load_generation.wrapping_add(1);
                self.invalidate_layout();
                self.reader_current_page = 0;
                self.confirm_clear_library = false;
                self.save_config();
            }

            if self.confirm_clear_library && ui.button("Cancel").clicked() {
                self.confirm_clear_library = false;
            }
        });

        ui.separator();

        egui::ScrollArea::vertical()
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                for book in self.books.clone() {
                    ui.group(|ui| {
                        ui.horizontal_top(|ui| {
                            self.show_book_cover(ui, &book);
                            ui.vertical(|ui| {
                                ui.label(format!("📖 {}", book.title));
                                ui.label(format!("👤 {}", book.author));

                                ui.horizontal(|ui| {
                                    if ui.button("Read in App").clicked() {
                                        self.start_reading(&book, ui.ctx().clone());
                                    }

                                    if let Some(path) = &book.path {
                                        if ui.button("Open Externally").clicked() {
                                            let _ = open::that(path);
                                        }
                                    }
                                });
                            });
                        });
                    });
                    ui.add_space(8.0);
                }
            });
    }

    fn show_reader_ui(&mut self, ui: &mut egui::Ui, book: &Book) {
        let lines_per_page = self.lines_per_page(ui.available_height());
        let chars_per_line = self.chars_per_line(ui.available_width());

        ui.horizontal(|ui| {
            if ui.button("Back to Library").clicked() {
                self.close_reader();
            }

            ui.separator();
            ui.label(format!("📖 {}", book.title));
            ui.label(format!("by {}", book.author));
        });

        ui.add_space(6.0);

        if self.reading_loading {
            ui.label("Loading book...");
            return;
        }

        if let Some(err) = &self.reading_error {
            ui.colored_label(egui::Color32::from_rgb(220, 80, 80), err);

            if let Some(path) = &book.path {
                if ui.button("Open This Book Externally").clicked() {
                    let _ = open::that(path);
                }
            }

            return;
        }

        if self.pdf_document.is_some() {
            self.show_pdf_reader_ui(ui);
            return;
        }

        ui.horizontal(|ui| {
            ui.label("Text size");
            let changed = ui
                .add(egui::Slider::new(&mut self.reader_font_size, 12.0..=32.0).suffix(" px"))
                .changed();

            if changed {
                self.reader_current_page = 0;
                self.save_config();
            }

            ui.separator();
            ui.label("Search");
            if ui.text_edit_singleline(&mut self.reader_search).changed() {
                self.reader_current_page = 0;
            }
        });

        ui.separator();

        self.request_layout(chars_per_line, ui.ctx().clone());
        if self.layout_lines.is_empty() && self.layout_pending.is_some() {
            ui.label("Formatting text...");
            return;
        }

        let total_matching_lines = self.layout_lines.len();
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

        let visible_lines = self.current_page_lines(lines_per_page);

        egui::ScrollArea::vertical()
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                for (line_number, line) in &visible_lines {
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::TOP), |ui| {
                        ui.add_sized(
                            [48.0, line.font_size(self.reader_font_size) + 6.0],
                            egui::Label::new(
                                egui::RichText::new(line_number.to_string())
                                    .size(12.0)
                                    .color(egui::Color32::GRAY),
                            ),
                        );
                        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                            ui.label(line.rich_text(self.reader_font_size));
                        });
                    });
                    if line.style != ReaderTextStyle::Body {
                        ui.add_space(4.0);
                    }
                }
            });
    }

    fn show_pdf_reader_ui(&mut self, ui: &mut egui::Ui) {
        let page_count = self
            .pdf_document
            .as_ref()
            .map(|document| document.page_count)
            .unwrap_or(1);
        self.reader_current_page = self.reader_current_page.min(page_count.saturating_sub(1));
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

        if ui.input(|input| input.key_pressed(egui::Key::ArrowLeft)) && can_go_back {
            self.reader_current_page = self.reader_current_page.saturating_sub(1);
            self.save_config();
        }
        if ui.input(|input| input.key_pressed(egui::Key::ArrowRight)) && can_go_forward {
            self.reader_current_page += 1;
            self.save_config();
        }

        self.request_pdf_page(ui.ctx().clone());
        if self.pdf_page_texture.as_ref().map(|(page, _)| *page) != Some(self.reader_current_page) {
            ui.label("Rendering PDF page...");
            return;
        }

        let Some((_, texture)) = &self.pdf_page_texture else {
            return;
        };
        let available = ui.available_size();
        let texture_size = texture.size_vec2();
        let scale = (available.x / texture_size.x)
            .min(available.y / texture_size.y)
            .min(1.0);
        let display_size = texture_size * scale;

        egui::ScrollArea::both()
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                ui.image((texture.id(), display_size));
            });
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReaderTextStyle {
    Body,
    Title,
    Heading1,
    Heading2,
    Heading3,
}

impl ReaderTextStyle {
    fn code(self) -> &'static str {
        match self {
            Self::Body => "body",
            Self::Title => "title",
            Self::Heading1 => "h1",
            Self::Heading2 => "h2",
            Self::Heading3 => "h3",
        }
    }

    fn from_code(code: &str) -> Self {
        match code {
            "title" => Self::Title,
            "h1" => Self::Heading1,
            "h2" => Self::Heading2,
            "h3" => Self::Heading3,
            _ => Self::Body,
        }
    }
}

#[derive(Clone)]
struct ReaderLine {
    text: String,
    style: ReaderTextStyle,
}

impl ReaderLine {
    fn new(text: String, style: ReaderTextStyle) -> Self {
        Self { text, style }
    }

    fn font_size(&self, body_size: f32) -> f32 {
        match self.style {
            ReaderTextStyle::Body => body_size,
            ReaderTextStyle::Title => body_size * 1.8,
            ReaderTextStyle::Heading1 => body_size * 1.6,
            ReaderTextStyle::Heading2 => body_size * 1.4,
            ReaderTextStyle::Heading3 => body_size * 1.2,
        }
    }

    fn rich_text(&self, body_size: f32) -> egui::RichText {
        let text = egui::RichText::new(&self.text).size(self.font_size(body_size));
        if self.style == ReaderTextStyle::Body {
            text
        } else {
            text.strong()
        }
    }
}

impl eframe::App for EbookApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_background_jobs(ctx);
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
    use std::fs;

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
                    <title>Hidden metadata</title>
                    <style>@page { margin-bottom: 5pt; }</style>
                </head>
                <body>
                    <p>Hello world.</p>
                </body>
            </html>
        "#;

        let text = EbookApp::html_to_text(html);

        assert!(text.contains("Hello world."));
        assert!(!text.contains("Hidden metadata"));
        assert!(!text.contains("@page"));
        assert!(!text.contains("margin-bottom"));
    }

    #[test]
    fn page_count_rounds_up_and_never_returns_zero() {
        assert_eq!(EbookApp::page_count(0, 20), 1);
        assert_eq!(EbookApp::page_count(20, 20), 1);
        assert_eq!(EbookApp::page_count(21, 20), 2);
    }

    #[test]
    fn wrapped_lines_split_long_epub_paragraphs_for_pagination() {
        let text = "A long EPUB paragraph needs to become several visible reader lines.";

        let lines = EbookApp::wrapped_lines(text, 16);

        assert!(lines.len() > 1);
        assert!(lines.iter().all(|line| line.text.chars().count() <= 16));
        assert_eq!(
            lines
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>()
                .join(" "),
            text
        );
    }

    #[test]
    fn html_to_text_preserves_heading_style() {
        let text = EbookApp::html_to_text("<h1>Chapter One</h1><p>Body text.</p>");
        let lines = EbookApp::wrapped_lines(&text, 80);

        assert_eq!(lines[0].text, "Chapter One");
        assert!(lines[0].font_size(18.0) > lines[1].font_size(18.0));
    }

    #[test]
    fn decode_ppm_builds_pdf_page_image() {
        let image = EbookApp::decode_ppm(b"P6\n# comment\n2 1\n255\n\x20\x00\x00\x00\xff\x00")
            .expect("valid PPM image");

        assert_eq!(image.size, [2, 1]);
        assert_eq!(image.pixels.len(), 2);
    }

    #[test]
    fn cover_cache_key_changes_when_book_changes() {
        let path = std::env::temp_dir().join(format!(
            "booklauncher-cover-key-test-{}.epub",
            std::process::id()
        ));
        fs::write(&path, b"first").expect("write first test book");
        let first = EbookApp::cover_key(&path).expect("first cache key");

        fs::write(&path, b"second version").expect("write updated test book");
        let second = EbookApp::cover_key(&path).expect("second cache key");
        let _ = fs::remove_file(path);

        assert_ne!(first, second);
    }
}
