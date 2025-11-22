use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::Path;
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone)]
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

        Some(Book {
            title: title.replace('_', " "),
            author: author.replace('_', " "),
            path: Some(path.to_path_buf()),
        })
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
                // Add or remove extensions as you like
                if ["epub", "mobi", "azw3", "pdf"].contains(&ext.as_str()) {
                    if let Some(book) = Book::from_filename(&path) {
                        books.push(book);
                    }
                }
            }
        }

        Ok(books) // <-- important: return the Vec
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
