use anyhow::Result;
use epub::doc::EpubDoc;
use image::codecs::png::PngEncoder;
use image::imageops::FilterType;
use image::io::Reader as ImageReader;
// use lopdf::Document; // Remove or comment out if not using lopdf
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::io::Cursor;
use std::io::Write;
use std::path::{Path, PathBuf};

// For PDF
use pdf_extract;

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
    #[serde(skip)]
    pub cover_bytes: Option<Vec<u8>>,
}

impl Book {
    /// Create a Book from a filename, doing basic parsing for title/author.
    /// Tries "Title - Author.ext" first; otherwise uses the stem as title and "Unknown" author.
    /// Does not load metadata or cover yet (deferred to extract_metadata).
    pub fn from_filename(path: &Path) -> Option<Self> {
        let filename_stem = path.file_stem()?.to_string_lossy().to_string();

        let (title, author) = if let Some((t, a)) = filename_stem.split_once(" - ") {
            (t.trim().to_string(), a.trim().to_string())
        } else {
            (filename_stem, "Unknown".to_string())
        };

        Some(Book {
            title: title.replace('_', " "),
            author: author.replace('_', " "),
            path: Some(path.to_path_buf()),
            cover_bytes: None,
        })
    }
    pub fn extract_metadata(&mut self) -> Result<()> {
        let path = match &self.path {
            Some(p) => p,
            None => return Ok(()),
        };

        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .map(str::to_lowercase)
            .unwrap_or_default();

        match ext.as_str() {
            "epub" => {
                let mut doc = EpubDoc::new(path)?;

                // epub::doc::mdata returns Option<&MetadataItem>
                if let Some(t) = doc.mdata("title") {
                    self.title = format!("{:?}", t);
                }
                if let Some(a) = doc.mdata("creator") {
                    self.author = format!("{:?}", a);
                }

                if let Some((cover_data, _)) = doc.get_cover() {
                    // returns Option<(Vec<u8>, String)>
                    let img = ImageReader::new(Cursor::new(cover_data))
                        .with_guessed_format()?
                        .decode()?;

                    let thumbnail = img.resize_exact(100, 140, FilterType::Triangle);

                    let mut png_bytes = Vec::new();
                    let encoder = PngEncoder::new(&mut png_bytes);
                    thumbnail.write_with_encoder(encoder)?;
                    self.cover_bytes = Some(png_bytes);
                }
            }

            "pdf" => {
                // PDF metadata extraction not implemented.
                // You can use pdf_extract or another crate for extracting text/metadata.
                // Cover extraction is non-trivial for PDFs.
            }

            "mobi" | "azw3" => {
                let mobi = Mobi::from_path(path)?;

                // mobi.title() → Option<&str>
                let title = mobi.title();
                self.title = if title.is_empty() { "Unknown".to_string() } else { title };
                // mobi.author() → Option<&str>
                if let Some(author) = mobi.author() {
                    self.author = author.to_string();
                }

                // Most mobi crates do NOT have .cover_image()
                // Popular crates like "mobi" (0.7.x) usually expose it via .raw_records()
                // or you need "mobi-python" bindings or "kindle-unpack" logic.
                // Simplest fix for now: skip cover or use first image record if available

                // Placeholder: no cover for now
                // self.cover_bytes = None;

                // If you switch to the "mobi" crate that supports it, example:
                // if let Some(cover) = mobi.get_cover_image() { ... }
            }

            _ => {}
        }

        Ok(())
    }
    pub fn fetch_missing_metadata(&mut self) -> Result<()> {
        if self.author != "Unknown" && self.cover_bytes.is_some() {
            return Ok(());
        }
        let query = encode(&self.title);
        let url = format!("https://openlibrary.org/search.json?title={}", query);
        let resp = get(&url)?.json::<serde_json::Value>()?;
        if let Some(docs) = resp.get("docs").and_then(|d| d.as_array()) {
            if let Some(first) = docs.get(0) {
                if self.author == "Unknown" {
                    if let Some(authors) = first.get("author_name").and_then(|an| an.as_array()) {
                        if let Some(a) = authors.get(0).and_then(|a| a.as_str()) {
                            self.author = a.to_string();
                        }
                    }
                }
                if self.cover_bytes.is_none() {
                    if let Some(cover_id) = first.get("cover_i").and_then(|ci| ci.as_i64()) {
                        let cover_url =
                            format!("https://covers.openlibrary.org/b/id/{}-M.jpg", cover_id);
                        let bytes = get(&cover_url)?.bytes()?;
                        let img = ImageReader::new(Cursor::new(bytes))
                            .with_guessed_format()?
                            .decode()?;
                        let thumbnail = img.resize_exact(100, 140, FilterType::Triangle);
                        let mut png_bytes = Vec::new();
                        let encoder = PngEncoder::new(&mut png_bytes);
                        thumbnail.write_with_encoder(encoder)?;
                        self.cover_bytes = Some(png_bytes);
                    }
                }
            }
        }
        Ok(())
    }

    /// Recursively scan a directory and collect all recognized book files.
    pub fn from_dir(dir: &Path) -> io::Result<Vec<Book>> {
        let mut books = Vec::new();

        let ext_str = dir
            .extension()
            .and_then(|s| s.to_str())
            .map(|s| s.to_lowercase());

        if let Some(ext) = ext_str {
            if ["epub", "mobi", "azw3", "pdf"].contains(&ext.as_str()) {
                if let Some(book) = Book::from_filename(dir) {
                    books.push(book);
                }
                return Ok(books);
            }
        }

        if !dir.exists() {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_parse_filename() {
        let p = PathBuf::from("/books/The_Lies_of_Locke_Lamora - Scott_Lynch.epub");
        let book = Book::from_filename(&p).unwrap();
        assert_eq!(book.title, "The Lies of Locke Lamora");
        assert_eq!(book.author, "Scott Lynch");
        assert_eq!(book.path.as_ref().unwrap(), &p);
    }
}
