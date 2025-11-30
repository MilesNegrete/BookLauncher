use rusqlite::{params, Connection, OptionalExtension, Result};
use std::path::Path;

use crate::book::Book;

/// ==========================================================
///  PUBLIC API
/// ==========================================================

/// Called on app startup.
/// Ensures database exists, creates tables if missing.
pub fn make_database(db_path: &Path) -> Result<Connection> {
    let first_time = !db_path.exists();
    let conn = Connection::open(db_path)?;

    if first_time {
        create_schema(&conn)?;
    }

    Ok(conn)
}

/// Loads all books where Exists=true.
pub fn database_startup(conn: &Connection) -> Result<Vec<Book>> {
    let mut stmt =
        conn.prepare("SELECT id, title, author, path FROM books WHERE exists_flag = 1")?;

    let rows = stmt.query_map([], |row| {
        Ok(Book {
            id: row.get(0)?,
            title: row.get(1)?,
            author: row.get(2)?,
            path: row.get::<_, String>(3)?.into(),
        })
    })?;

    let mut books = vec![];
    for r in rows {
        books.push(r?);
    }

    Ok(books)
}

/// Adds or updates a book record based on fuzzy matching.
pub fn add_to_database(conn: &Connection, book: &Book) -> Result<()> {
    if let Some(existing_id) = fuzzy_match(conn, &book.title, &book.author)? {
        // Book already exists in DB
        let exists: bool = conn.query_row(
            "SELECT exists_flag FROM books WHERE id = ?1",
            params![existing_id],
            |row| row.get(0),
        )?;

        if exists {
            // Already present & active → do nothing
            return Ok(());
        } else {
            // Found but previously deleted → mark as present again
            conn.execute(
                "UPDATE books SET exists_flag = 1, path = ?1 WHERE id = ?2",
                params![book.path.to_string_lossy(), existing_id],
            )?;
            return Ok(());
        }
    } else {
        // No match → insert new book
        conn.execute(
            "INSERT INTO books (title, author, path, exists_flag)
             VALUES (?1, ?2, ?3, 1)",
            params![book.title, book.author, book.path.to_string_lossy(),],
        )?;
    }

    Ok(())
}

/// Marks a book as deleted, but does NOT remove metadata.
pub fn make_exist_false(conn: &Connection, book_id: i64) -> Result<()> {
    conn.execute(
        "UPDATE books SET exists_flag = 0 WHERE id = ?1",
        params![book_id],
    )?;
    Ok(())
}

/// ==========================================================
///  INTERNAL HELPERS
/// ==========================================================

/// Creates the table on first run.
fn create_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS books (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            title       TEXT NOT NULL,
            author      TEXT NOT NULL,
            path        TEXT NOT NULL,
            exists_flag INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_books_title_author
            ON books(title, author);
        ",
    )?;
    Ok(())
}

/// Simple fuzzy match: normalized title+author comparison.
///
/// *Later we can improve this using levenshtein or trigram scoring.*
fn fuzzy_match(conn: &Connection, title: &str, author: &str) -> Result<Option<i64>> {
    let norm_title = normalize(title);
    let norm_author = normalize(author);

    conn.query_row(
        "
        SELECT id FROM books
        WHERE lower(title) = ?1 AND lower(author) = ?2
        ",
        params![norm_title, norm_author],
        |row| row.get(0),
    )
    .optional()
}

/// Extremely basic normalization
fn normalize(s: &str) -> String {
    s.trim().to_lowercase()
}
