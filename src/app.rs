use crate::book::Book;
use eframe::egui;

pub struct EbookApp {
    books: Vec<Book>,
}

impl Default for EbookApp {
    fn default() -> Self {
        Self {
            books: Book::sample_books(),
        }
    }
}

impl eframe::App for EbookApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("📚 My E-Book Library");
            ui.separator();

            for book in &self.books {
                ui.group(|ui| {
                    ui.label(format!("📖 {}", book.title));
                    ui.label(format!("👤 {}", book.author));
                });
                ui.add_space(8.0);
            }
        });
    }
}
