#![allow(unused_imports)]
use eframe::egui;
use rfd::FileDialog;
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::{fs, io};

use crate::book::Book;
use crate::database_maker::{add_to_database, database_startup, make_database};

pub struct App {
    books: Vec<Book>,
    last_dir: Option<PathBuf>,
    open_folder_picker: bool,
    db: Connection,
}

impl Default for App {
    fn default() -> Self {
        // 1. Open/create DB
        let db_path = Path::new("books.db");
        let conn = make_database(db_path).expect("Failed to open DB");

        // 2. Load books where exists_flag = true
        let books = database_startup(&conn).unwrap_or_default();

        Self {
            books,
            last_dir: None,
            open_folder_picker: false,
            db: conn,
        }
    }
}

impl App {
    fn add_book_from_path(&mut self, path: &Path) -> Result<(), String> {
        match Book::from_filename(path) {
            Some(mut book) => {
                // Update DB first
                if let Err(e) = add_to_database(&self.db, &book) {
                    return Err(format!("DB error: {e}"));
                }

                // The DB doesn't return the ID yet, so display anyway
                if !self.books.iter().any(|b| b.path == book.path) {
                    self.books.push(book);
                }

                Ok(())
            }
            None => Err(format!("Couldn’t load: {}", path.display())),
        }
    }

    fn scan_folder(&mut self, dir: &Path) -> io::Result<Vec<Book>> {
        let books = Book::from_dir(dir)?;

        for b in &books {
            // Add to database
            let _ = add_to_database(&self.db, b);

            // Add to UI list if not already present
            if !self.books.iter().any(|x| x.path == b.path) {
                self.books.push(b.clone());
            }
        }

        Ok(books)
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("📚 My E-Book Library");
            ui.separator();

            // Display list of books (from DB)
            for b in &self.books {
                ui.label(format!("{} — {}", b.title, b.author));
            }

            ui.separator();

            // Add Book (single file)
            if ui.button("Add Book (File)").clicked() {
                if let Some(path) = FileDialog::new().pick_file() {
                    self.last_dir = path.parent().map(|p| p.to_path_buf());
                    let _ = self.add_book_from_path(&path);
                }
            }

            // Select File (unused?)
            if ui.button("Select File").clicked() {
                self.open_folder_picker = true;
            }

            if self.open_folder_picker {
                if let Some(path) = FileDialog::new().pick_file() {
                    self.last_dir = path.parent().map(|p| p.to_path_buf());
                }
                self.open_folder_picker = false;
            }

            // Scan Folder button
            if ui.button("Scan Folder").clicked() {
                self.open_folder_picker = true;
            }

            if self.open_folder_picker {
                if let Some(folder) = FileDialog::new().pick_folder() {
                    self.last_dir = Some(folder.clone());

                    match self.scan_folder(&folder) {
                        Ok(_) => {}
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
