use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct Book {
    pub title: String,
    pub author: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
}

impl Book {
    /// Simple convenience list so `app.rs` builds without needing the FS yet.
    pub fn sample_books() -> Vec<Book> {
        vec![
            Book {
                title: "The Hobbit".to_string(),
                author: "J.R.R. Tolkien".to_string(),
                path: None,
            },
            Book {
                title: "Dune".to_string(),
                author: "Frank Herbert".to_string(),
                path: None,
            },
        ]
    }

    /// Create a Book from a filename, doing basic parsing for title/author.
    /// Tries "Title - Author.ext" first; otherwise uses the stem as title and "Unknown" author.
    pub fn from_filename(path: &Path) -> Option<Self> {
        let filename_stem = path.file_stem()?.to_string_lossy().to_string();

        let (title, author) = if let Some((t, a)) = filename_stem.split_once(" - ") {
            (t.trim().to_string(), a.trim().to_string())
        } else {
            (filename_stem, "Unknown".to_string())
        };

        let mut book = Book {
            title: title.replace('_', " "),
            author: author.replace('_', " "),
            path: Some(path.to_path_buf()),
        };
        book.apply_embedded_metadata();
        Some(book)
    }

    pub fn key(&self) -> Option<String> {
        self.path
            .as_deref()
            .map(|path| path.to_string_lossy().into_owned())
    }

    pub fn format(&self) -> String {
        self.path
            .as_deref()
            .and_then(Path::extension)
            .and_then(|ext| ext.to_str())
            .unwrap_or("unknown")
            .to_ascii_lowercase()
    }

    fn apply_embedded_metadata(&mut self) {
        let Some(path) = self.path.as_deref() else {
            return;
        };
        if path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref()
            != Some("epub")
        {
            return;
        }

        let Ok(doc) = epub::doc::EpubDoc::new(path) else {
            return;
        };
        if let Some(title) = doc.get_title().filter(|title| !title.trim().is_empty()) {
            self.title = title;
        }
        if let Some(author) = doc
            .mdata("creator")
            .map(|metadata| metadata.value.clone())
            .filter(|author| !author.trim().is_empty())
        {
            self.author = author;
        }
    }

    /// Recursively scan a directory and collect all recognized book files.
    pub fn from_dir(dir: &Path) -> io::Result<Vec<Book>> {
        let mut books = Vec::new();

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
                // Recurse into subdirectories
                books.extend(Self::from_dir(&path)?);
            } else if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
                let ext = ext.to_lowercase();
                if ["epub", "mobi", "azw3", "pdf", "txt", "md"].contains(&ext.as_str()) {
                    if let Some(book) = Book::from_filename(&path) {
                        books.push(book);
                    }
                }
            }
        }

        books.sort_by_cached_key(|book| book.title.to_lowercase());
        books.dedup_by(|left, right| left.path == right.path);
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

    #[test]
    fn book_key_and_format_come_from_path() {
        let book = Book {
            title: "Example".to_string(),
            author: "Author".to_string(),
            path: Some(PathBuf::from("/books/Example.PDF")),
        };

        assert_eq!(book.key().as_deref(), Some("/books/Example.PDF"));
        assert_eq!(book.format(), "pdf");
    }
}
