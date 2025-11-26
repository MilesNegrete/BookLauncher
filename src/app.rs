use eframe::egui;
use rfd::FileDialog;
use std::path::{Path, PathBuf};
use std::{fs, io};

use crate::book::Book;

pub struct App {
    books: Vec<Book>,
    last_dir: Option<PathBuf>,
    open_folder_picker: bool,
}

impl Default for App {
    fn default() -> Self {
        Self {
            books: Vec::new(),
            last_dir: None,
            open_folder_picker: false,
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

    fn scan_folder(&self, dir: &Path) -> io::Result<Vec<Book>> {
        Book::from_dir(dir)
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("📚 My E-Book Library");
            ui.separator();

            for b in &self.books {
                ui.label(format!("{} — {}", b.title, b.author));
            }

            ui.separator();

            if ui.button("Add Book (File)").clicked() {
                if let Some(path) = FileDialog::new().pick_file() {
                    self.last_dir = path.parent().map(|p| p.to_path_buf());
                    let _ = self.add_book_from_path(&path);
                }
            }

            if ui.button("Select File").clicked() {
                self.open_folder_picker = true;
            }

            if self.open_folder_picker {
                if let Some(path) = FileDialog::new().pick_file() {
                    self.last_dir = path.parent().map(|p| p.to_path_buf());
                }
                self.open_folder_picker = false;
            }

            if ui.button("Scan Folder").clicked() {
                self.open_folder_picker = true;
            }

            if self.open_folder_picker {
                if let Some(folder) = FileDialog::new().pick_folder() {
                    self.last_dir = Some(folder.clone());

                    match self.scan_folder(&folder) {
                        Ok(found_books) => {
                            for b in found_books {
                                // push if not duplicate
                                if !self.books.iter().any(|x| x.path == b.path) {
                                    self.books.push(b);
                                }
                            }
                        }
                        Err(e) => {
                            ui.label(format!("Error scanning: {}", e));
                        }
                    }
                }

                self.open_folder_picker = false;
            }
        });
    }
}
