use eframe::egui;
use rfd::FileDialog;
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

// Use the Book from book.rs
use crate::book::Book;

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
    Update(PathBuf, String, String),
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
    #[allow(dead_code)]
    fn add_book_from_path(&mut self, path: &Path) -> Result<(), String> {
        println!("[DEBUG] Attempting to add book: {:?}", path);
        match Book::from_filename(path) {
            Some(book) => {
                let already = self.books.iter().any(|b| b.path == book.path);
                if !already {
                    println!("[DEBUG] Successfully added: {}", book.title);
                    self.books.push(book);
                    Ok(())
                } else {
                    println!("[DEBUG] Duplicate book found, skipping: {:?}", path);
                    Err("That book is already in your library.".to_string())
                }
            }
            None => {
                println!("[DEBUG] Failed to load book from: {:?}", path);
                Err(format!("Couldn’t load: {}", path.display()))
            }
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        
        // --- ASYNC DATA HANDLING DEBUG ---
        // Check if the scan thread has finished
        if let Some(rx) = &self.scan_rx {
            if let Ok(result) = rx.try_recv() {
                match result {
                    Ok(new_books) => {
                        println!("[DEBUG] Scan complete. Found {} books.", new_books.len());
                        self.books.extend(new_books);
                    }
                    Err(e) => println!("[DEBUG] Scan error: {}", e),
                }
                self.scan_rx = None; // Reset the receiver
            }
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("📚 My E-Book Library");

            // UI Debug Info (Optional toggle for development)
            #[cfg(debug_assertions)]
            {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("DEBUG MODE").color(egui::Color32::RED).small());
                    if ui.button("Print State to Console").clicked() {
                        println!("[DEBUG] Current Library Count: {}", self.books.len());
                        println!("[DEBUG] Last Directory: {:?}", self.last_dir);
                    }
                });
            }

            ui.separator();

            if self.scan_rx.is_some() {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Scanning folder…");
                });
            }

            if self.metadata_rx.is_some() {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Extracting metadata…");
                });
            }

            ui.separator();

            let num_books = self.books.len();

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .max_height(ui.available_height() - 100.0) 
                .show(ui, |ui| {
                    if num_books == 0 {
                        ui.centered_and_justified(|ui| {
                            ui.label("No books yet.");
                        });
                    } else {
                        for book in &self.books {
                            ui.group(|ui| {
                                ui.vertical(|ui| {
                                    ui.strong(&book.title);
                                    ui.label(egui::RichText::new(format!("by {}", book.author)).italics());
                                });
                            });
                            ui.add_space(4.0);
                        }
                    }
                });

            ui.separator();

            ui.horizontal(|ui| {
                if ui.button("Scan Folder").clicked() {
                    println!("[DEBUG] Scan Folder button clicked");
                    self.open_folder_picker = true;
                }

                if ui.button("Clear Library").clicked() {
                    println!("[DEBUG] Library cleared");
                    self.books.clear();
                }
            });

            // Folder picker logic
            if self.open_folder_picker {
                self.open_folder_picker = false; // Reset early to avoid loop
                if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                    println!("[DEBUG] Folder selected: {:?}", folder);
                    self.last_dir = Some(folder.clone());
                    
                    let (tx, rx) = std::sync::mpsc::channel();
                    self.scan_rx = Some(rx);

                    let folder_clone = folder.clone();
                    let ctx_clone = ctx.clone(); // Needed to wake up UI thread when done
                    
                    std::thread::spawn(move || {
                        println!("[DEBUG] Starting background scan thread...");
                        let result = crate::book::Book::from_dir(&folder_clone);
                        let _ = tx.send(result);
                        ctx_clone.request_repaint(); // Tell egui to check the receiver
                        println!("[DEBUG] Background scan thread finished.");
                    });
                } else {
                    println!("[DEBUG] Folder picker cancelled.");
                }
            }
        });
    }
}