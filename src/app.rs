use crate::book::Book;
use eframe::egui;
use rfd::FileDialog;

pub struct EbookApp {
    books: Vec<Book>,
}

impl Default for EbookApp {
    fn default() -> Self {
        let dir = std::path::Path::new("/home/mjnegrete/Downloads"); // <- set your folder
        let books = Book::from_dir(dir).unwrap_or_else(|_| Book::sample_books());
        Self { books }
    }
}

impl eframe::App for EbookApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("📚 My E-Book Library");
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
