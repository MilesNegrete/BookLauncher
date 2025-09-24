use std::path::{Path, PathBuf};

use anyhow::Result;
use gio::prelude::*;
use glib::{self, ControlFlow};
use libadwaita as adw; // <- alias the crate so we can write `adw::...`
use adw::prelude::*;
use gtk4 as gtk;
use gtk::prelude::*;
use walkdir::WalkDir;

#[derive(Clone, Debug)]
struct Book {
    title: String,
    author: String,
    path: PathBuf,
}

fn main() -> glib::ExitCode {
    adw::init().expect("Failed to init Libadwaita");
    let app = adw::Application::builder()
        .application_id("com.example.storysphere")
        .build();

    app.connect_activate(build_ui);
    app.run()
}

fn build_ui(app: &adw::Application) {
    // Header
    let header = adw::HeaderBar::builder()
        .title_widget(&gtk::Label::new(Some("StorySphere (GTK)")))
        .build();

    // A plain button that opens a FileDialog (GTK4 replacement for FileChooserButton)
    let folder_btn = gtk::Button::with_label("Choose folder…");
    header.pack_start(&folder_btn);

    // List + scroller
    let list = gtk::ListBox::new();
    list.set_vexpand(true);

    let scroller = gtk::ScrolledWindow::builder()
        .child(&list)
        .vexpand(true)
        .hexpand(true)
        .build();

    let status = gtk::Label::new(Some("Pick a folder to begin…"));
    status.set_wrap(true);
    status.add_css_class("dim-label");

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(12)
        .margin_end(12)
        .build();

    content.append(&scroller);
    content.append(&status);

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .default_width(800)
        .default_height(600)
        .content(&content)
        .title("StorySphere (GTK)")
        .build();

    window.set_titlebar(Some(&header));
    window.present();

    // Channel to push scanned results back to main thread
    let (sender, receiver) =
        glib::MainContext::channel::<Result<Vec<Book>>>(glib::Priority::default());

    // Update UI when results arrive
    let list_clone = list.clone();
    let status_clone = status.clone();
    receiver.attach(None, move |result| {
        // Clear rows
        for child in list_clone.children() {
            list_clone.remove(&child);
        }

        match result {
            Ok(books) if !books.is_empty() => {
                for b in books {
                    let row = gtk::ListBoxRow::new();
                    let boxx = gtk::Box::new(gtk::Orientation::Vertical, 0);

                    let title_lbl = gtk::Label::new(Some(&b.title));
                    title_lbl.set_xalign(0.0);
                    title_lbl.add_css_class("title-3");

                    let subtitle = format!("{} — {}", b.author, b.path.display());
                    let sub_lbl = gtk::Label::new(Some(&subtitle));
                    sub_lbl.set_xalign(0.0);
                    sub_lbl.add_css_class("dim-label");
                    sub_lbl.set_wrap(true);

                    boxx.append(&title_lbl);
                    boxx.append(&sub_lbl);
                    row.set_child(Some(&boxx));
                    list_clone.append(&row);
                }
                status_clone.set_label("Done.");
            }
            Ok(_) => status_clone.set_label("No EPUBs found in that folder."),
            Err(e) => status_clone.set_label(&format!("Scan failed: {e}")),
        }
        ControlFlow::Continue
    });

    // Folder picker using FileDialog
    let win_weak = window.downgrade();
    let sender_for_btn = sender.clone();
    folder_btn.connect_clicked(move |_| {
        if let Some(win) = win_weak.upgrade() {
            let dialog = gtk::FileDialog::builder()
                .title("Choose a folder to scan")
                .build();

            dialog.select_folder(
                Some(&win),
                None::<&gio::Cancellable>,
                {
                    let sender = sender_for_btn.clone();
                    move |res| {
                        match res {
                            Ok(folder) => {
                                if let Some(path) = folder.path() {
                                    // Scan off the main thread
                                    std::thread::spawn(move || {
                                        let res = scan_epubs(&path);
                                        let _ = sender.send(res);
                                    });
                                }
                            }
                            Err(err) => {
                                // User canceled or error; ignore or log
                                eprintln!("Folder select error: {err}");
                            }
                        }
                    }
                },
            );
        }
    });
}

fn scan_epubs(root: &Path) -> Result<Vec<Book>> {
    let mut out = Vec::new();

    for entry in WalkDir::new(root).into_iter().filter_map(Result::ok) {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
            if ext.eq_ignore_ascii_case("epub") {
                match extract_epub_metadata(path) {
                    Ok(mut b) => {
                        b.path = path.to_path_buf();
                        out.push(b);
                    }
                    Err(_) => out.push(Book {
                        title: path
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or("Unknown")
                            .to_string(),
                        author: "Unknown".to_string(),
                        path: path.to_path_buf(),
                    }),
                }
            }
        }
    }

    out.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
    Ok(out)
}

fn extract_epub_metadata(path: &Path) -> Result<Book> {
    let doc = epub::doc::EpubDoc::new(path).map_err(|e| anyhow::anyhow!("{e}"))?;
    let title = doc.mdata("title").unwrap_or_else(|| {
        path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Unknown")
            .to_string()
    });
    let author = doc.mdata("creator").unwrap_or_else(|| "Unknown".to_string());
    Ok(Book {
        title,
        author,
        path: path.to_path_buf(),
    })
}
