use eframe::egui;
use rfd::FileDialog;
use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

// Use the Book from book.rs
use crate::book::Book;

//#[derive(Serialize, Clone)]
pub struct App {
    books: Vec<Book>,
    last_dir: Option<PathBuf>,
    open_folder_picker: bool,
    scan_rx: Option<Receiver<io::Result<Vec<Book>>>>,
    metadata_rx: Option<Receiver<BookUpdate>>,
    metadata_tx: Option<Sender<BookUpdate>>,
    done_threads: Vec<String>,
}

enum BookUpdate {
    Update(PathBuf, String, String, Option<Vec<u8>>),
    Done(String),
}

impl Default for App {
    fn default() -> Self {
        Self {
            books: Vec::new(),
            last_dir: None,
            open_folder_picker: false,
            scan_rx: None,
            metadata_rx: None,
            metadata_tx: None,
            done_threads: Vec::new(),
        }
    }
}

impl App {
    fn add_book_from_path(&mut self, path: &Path) -> Result<(), String> {
        match Book::from_filename(path) {
            Some(book) => {
                let already = self.books.iter().any(|b| b.path == book.path);
                if !already {
                    self.books.push(book);
                    Ok(())
                } else {
                    Err("That book is already in your library.".to_string())
                }
            }
            None => Err(format!("Couldn’t load: {}", path.display())),
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("📚 My E-Book Library");
            ui.separator();

            // Make the list scrollable
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .max_height(ui.available_height() - 100.0) // leave room for buttons
                .show(ui, |ui| {
                    if self.books.is_empty() {
                        ui.label("No books yet. Add some using the buttons below.");
                    } else {
                        for book in &self.books {
                            ui.horizontal(|ui| {
                                // Show cover if available
                                if let Some(cover) = &book.cover_bytes {
                                    let image = egui::ColorImage::from_rgba_unmultiplied([100, 140], &cover.clone());
                                    let texture = ctx.load_texture("book_cover", image, Default::default());
                                    ui.image(&texture);
                                } else {
                                    // Placeholder
                                    ui.label("[No cover]");
                                }

                                ui.vertical(|ui| {
                                    ui.strong(&book.title);
                                    ui.label(format!("by {}", book.author));
                                    if let Some(path) = &book.path {
                                        ui.label(path.display().to_string());
                                    }
                                });
                            });
                            ui.separator();
                        }
                    }
                });

            ui.separator();

            if ui.button("Scan Folder").clicked() {
                self.open_folder_picker = true;
            }

            if self.open_folder_picker {
                if let Some(folder) = FileDialog::new().pick_folder() {
                    self.last_dir = Some(folder.clone());
                    let (tx, rx) = mpsc::channel();
                    self.scan_rx = Some(rx);
                    let folder_clone = folder.clone();
                    thread::spawn(move || {
                        let result = Book::from_dir(&folder_clone);
                        tx.send(result).unwrap();
                    });
                    self.open_folder_picker = false;
                }
            }

            // Poll scan receiver
            if let Some(rx) = &self.scan_rx {
                if let Ok(result) = rx.try_recv() {
                    self.scan_rx = None; // Clear receiver
                    match result {
                        Ok(found_books) => {
                            // Thread 0: Count the number of books
                            let count = found_books.len();
                            // Could display count in UI if desired, e.g., ui.label(format!("Found {} books", count));

                            // Thread 1: Sort the books based on type (group by extension)
                            let mut groups: HashMap<String, Vec<Book>> = HashMap::new();
                            for mut book in found_books {
                                let ext = book
                                    .path
                                    .as_ref()
                                    .and_then(|p| p.extension())
                                    .and_then(|s| s.to_str())
                                    .map(|s| s.to_lowercase())
                                    .unwrap_or("unknown".to_string());
                                groups.entry(ext).or_insert_with(Vec::new).push(book);
                            }

                            // Thread 2: Add the books to the library
                            for (_, bs) in &groups {
                                for book in bs {
                                    if !self.books.iter().any(|x| x.path == book.path) {
                                        self.books.push(book.clone());
                                    }
                                }
                            }
                            // UI will update with new books on next frame

                            // Prepare for metadata extraction threads
                            let (tx, rx) = mpsc::channel();
                            self.metadata_rx = Some(rx);
                            self.metadata_tx = Some(tx.clone());
                            self.done_threads.clear();

                            // Clone groups for threads
                            let epubs = groups.get("epub").cloned().unwrap_or_default();
                            let mobis = groups.get("mobi").cloned().unwrap_or_default();
                            let azw3s = groups.get("azw3").cloned().unwrap_or_default();
                            let pdfs = groups.get("pdf").cloned().unwrap_or_default();

                            // Thread 3: Metadata extraction for epubs, then mobi/azw3
                            let tx3 = tx.clone();
                            thread::spawn(move || {
                                // Epubs first
                                for mut book in epubs {
                                    let _ = book.extract_metadata();
                                    tx3.send(BookUpdate::Update(
                                        book.path.clone().unwrap(),
                                        book.title.clone(),
                                        book.author.clone(),
                                        book.cover_bytes.clone(),
                                    ))
                                    .unwrap();
                                }
                                // Then mobi/azw3
                                for mut book in mobis.into_iter().chain(azw3s.into_iter()) {
                                    let _ = book.extract_metadata();
                                    tx3.send(BookUpdate::Update(
                                        book.path.clone().unwrap(),
                                        book.title.clone(),
                                        book.author.clone(),
                                        book.cover_bytes.clone(),
                                    ))
                                    .unwrap();
                                }
                                tx3.send(BookUpdate::Done("thread3".to_string())).unwrap();
                            });

                            // Thread 4: Metadata extraction for pdfs
                            let tx4 = tx.clone();
                            thread::spawn(move || {
                                for mut book in pdfs {
                                    let _ = book.extract_metadata();
                                    tx4.send(BookUpdate::Update(
                                        book.path.clone().unwrap(),
                                        book.title.clone(),
                                        book.author.clone(),
                                        book.cover_bytes.clone(),
                                    ))
                                    .unwrap();
                                }
                                tx4.send(BookUpdate::Done("thread4".to_string())).unwrap();
                            });
                        }
                        Err(e) => {
                            ui.label(format!("Error scanning: {}", e));
                        }
                    }
                }
            }

            // Poll metadata receiver
            if let Some(rx) = &self.metadata_rx {
                while let Ok(update) = rx.try_recv() {
                    match update {
                        BookUpdate::Update(path, title, author, cover_bytes) => {
                            if let Some(b) = self
                                .books
                                .iter_mut()
                                .find(|b| b.path.as_ref() == Some(&path))
                            {
                                b.title = title;
                                b.author = author;
                                b.cover_bytes = cover_bytes;
                            }
                            ctx.request_repaint(); // Request repaint to update UI
                        }
                        BookUpdate::Done(thread_id) => {
                            self.done_threads.push(thread_id);
                            if self.done_threads.contains(&"thread3".to_string())
                                && self.done_threads.contains(&"thread4".to_string())
                            {
                                // Thread 5: Ping openlibrary API for missing metadata
                                let tx5 = self.metadata_tx.as_ref().unwrap().clone();
                                let books_clone = self.books.clone();
                                thread::spawn(move || {
                                    for mut book in books_clone {
                                        if book.author == "Unknown" || book.cover_bytes.is_none() {
                                            let _ = book.fetch_missing_metadata();
                                            tx5.send(BookUpdate::Update(
                                                book.path.clone().unwrap(),
                                                book.title.clone(),
                                                book.author.clone(),
                                                book.cover_bytes.clone(),
                                            ))
                                            .unwrap();
                                        }
                                    }
                                    tx5.send(BookUpdate::Done("thread5".to_string())).unwrap();
                                });
                            }
                        }
                    }
                }
            }
        });
    }
}
