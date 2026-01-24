use crate::book::Book;
use eframe::egui;
use rfd::FileDialog;
use std::path::PathBuf;
use directories::ProjectDirs;
use serde_json;


pub struct EbookApp {
    books: Vec<Book>,
    library_path: Option<PathBuf>,
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct AppConfig {
    library_path: Option<PathBuf>,
    added_books: Vec<Book>,
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

        if let Ok(txt) = std::fs::read_to_string(&config_path) {
            if let Ok(cfg) = serde_json::from_str::<AppConfig>(&txt) {
                books = cfg.added_books;
                library_path = cfg.library_path;
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
        }
    }
}

impl eframe::App for EbookApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("📚 My E-Book Library");
            ui.separator();

            if ui.button("Choose Library Folder").clicked() {
                if let Some(folder) = FileDialog::new().pick_folder() {
                    self.library_path = Some(folder.clone());

                    if let Ok(new_books) = Book::from_dir(&folder) {
                        self.books = new_books;
                    }

                    // Save config
                    let config = AppConfig {
                        library_path: self.library_path.clone(),
                        added_books: self.books.clone(),
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
            }

            ui.separator();

            egui::ScrollArea::vertical()
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    for book in &self.books {
                        ui.group(|ui| {
                            ui.label(format!("📖 {}", book.title));
                            ui.label(format!("👤 {}", book.author));
                        });
                        ui.add_space(8.0);
                    }
                });

            ui.separator();

            if ui.button("Select File").clicked() {
                if let Some(path) = FileDialog::new()
                    .add_filter("Ebook", &["epub", "json"])
                    .pick_file()
                {
                    println!("Selected file: {:?}", path);

                    if let Some(book) = Book::from_filename(&path) {
                        self.books.push(book);
                    }
                }
            }
        });
    }
}
