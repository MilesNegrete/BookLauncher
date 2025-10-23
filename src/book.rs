use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone)]
pub struct Book {
    pub title: String,
    pub author: String,
}

impl Book {
    pub fn sample_books() -> Vec<Book> {
        vec![
            Book {
                title: "Amber and Iron".into(),
                author: "Margaret Weis".into(),
            },
            Book {
                title: "AI Engineering: Building Applications with FOundation Models".into(),
                author: "Chip Huyen".into(),
            },
            Book {
                title: "The Absolute Guide to Dashboarding and Reporting with Power BI".into(),
                author: "Kasper de Jonge".into(),
            },
        ]
    }
}
