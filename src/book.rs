use anyhow::Result;
use epub::doc::EpubDoc;
use image::codecs::png::PngEncoder;
use image::imageops::FilterType;
#[allow(deprecated)]
use image::io::Reader as ImageReader;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::io::Cursor;
use std::path::{Path, PathBuf};

// For Mobi
use mobi::Mobi;

// For API
use reqwest::blocking::get;
use urlencoding::encode;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Book {
    pub title: String,
    pub author: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
}

impl Book {
    pub fn from_filename(path: &Path) -> Option<Self> {
        // DEBUG: Identify what file we are starting with
        println!("[DEBUG] Parsing filename for: {:?}", path);

        let filename_stem = path.file_stem()?.to_string_lossy().to_string();

        let (title, author) = if let Some((t, a)) = filename_stem.split_once(" - ") {
            (t.trim().to_string(), a.trim().to_string())
        } else {
            // DEBUG: Note when the expected "Title - Author" format isn't found
            println!("[DEBUG] Filename '{}' does not follow 'Title - Author' format. Using fallback.", filename_stem);
            (filename_stem, "Unknown".to_string())
        };

        Some(Book {
            title: title.replace('_', " "),
            author: author.replace('_', " "),
            path: Some(path.to_path_buf()),
        })
    }

    pub fn extract_metadata(&mut self) -> Result<()> {
        let path = match &self.path {
            Some(p) => p,
            None => {
                println!("[DEBUG] extract_metadata called on book with no path.");
                return Ok(());
            }
        };

        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .map(str::to_lowercase)
            .unwrap_or_default();

        println!("[DEBUG] Extracting internal metadata for {} (Type: {})", path.display(), ext);

        match ext.as_str() {
            "epub" => {
                let mut doc = EpubDoc::new(path)?;

                if let Some(t) = doc.mdata("title") {
                    println!("[DEBUG] Found EPUB title: {:?}", t);
                    self.title = format!("{:?}", t);
                }
                if let Some(a) = doc.mdata("creator") {
                    println!("[DEBUG] Found EPUB creator: {:?}", a);
                    self.author = format!("{:?}", a);
                }

                if let Some((cover_data, _)) = doc.get_cover() {
                    println!("[DEBUG] Found EPUB cover ({} bytes)", cover_data.len());
                    #[allow(deprecated)]
                    let img = ImageReader::new(Cursor::new(cover_data))
                        .with_guessed_format()?
                        .decode()?;

                    let thumbnail = img.resize_exact(100, 140, FilterType::Triangle);
                    let mut png_bytes = Vec::new();
                    let encoder = PngEncoder::new(&mut png_bytes);
                    thumbnail.write_with_encoder(encoder)?;
                }
            }

            "mobi" | "azw3" => {
                let mobi = Mobi::from_path(path)?;
                let title = mobi.title();
                println!("[DEBUG] Found MOBI/AZW3 title: {}", title);
                self.title = if title.is_empty() { "Unknown".to_string() } else { title };
                
                if let Some(author) = mobi.author() {
                    println!("[DEBUG] Found MOBI/AZW3 author: {}", author);
                    self.author = author.to_string();
                }
            }

            _ => {
                println!("[DEBUG] No specialized metadata extractor for extension: {}", ext);
            }
        }

        Ok(())
    }

    pub fn fetch_missing_metadata(&mut self) -> Result<()> {
        if self.author != "Unknown" {
            return Ok(());
        }

        println!("[DEBUG] Author unknown for '{}'. Fetching from OpenLibrary API...", self.title);
        
        let query = encode(&self.title);
        let url = format!("https://openlibrary.org/search.json?title={}", query);
        
        let resp = get(&url)?.json::<serde_json::Value>()?;
        if let Some(docs) = resp.get("docs").and_then(|d| d.as_array()) {
            if let Some(first) = docs.get(0) {
                if let Some(authors) = first.get("author_name").and_then(|an| an.as_array()) {
                    if let Some(a) = authors.get(0).and_then(|a| a.as_str()) {
                        println!("[DEBUG] API match found! Author: {}", a);
                        self.author = a.to_string();
                    }
                }
            } else {
                println!("[DEBUG] API returned no documents for title: {}", self.title);
            }
        }
        Ok(())
    }

    pub fn from_dir(dir: &Path) -> io::Result<Vec<Book>> {
        println!("[DEBUG] Scanning directory: {:?}", dir);
        let mut books = Vec::new();

        if !dir.exists() {
            println!("[DEBUG] ERROR: Directory does not exist: {:?}", dir);
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("Directory not found: {}", dir.display()),
            ));
        }

        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_dir() {
                books.extend(Self::from_dir(&path)?);
            } else if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
                let ext = ext.to_lowercase();
                if ["epub", "mobi", "azw3", "pdf"].contains(&ext.as_str()) {
                    if let Some(book) = Book::from_filename(&path) {
                        books.push(book);
                    }
                }
            }
        }

        Ok(books)
    }
}