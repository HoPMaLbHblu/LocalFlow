pub mod api;
pub mod engine;
pub mod sandbox;

use serde::Serialize;

/// A ready-to-use automation offered on the "New automation" page.
#[derive(Debug, Clone, Serialize)]
pub struct Example {
    pub slug: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub schedule: &'static str,
    pub code: &'static str,
}

pub const EXAMPLES: &[Example] = &[
    Example {
        slug: "hello-world",
        title: "Hello world",
        description: "The smallest possible automation. A good place to start.",
        schedule: "",
        code: include_str!("../../examples/hello_world.lua"),
    },
    Example {
        slug: "organize-pdfs",
        title: "Organize PDF files",
        description: "Move PDFs from Downloads into Documents/PDF.",
        schedule: "0 0 * * * *",
        code: include_str!("../../examples/organize_pdfs.lua"),
    },
    Example {
        slug: "tidy-screenshots",
        title: "Tidy screenshots",
        description: "Move screenshots off the Desktop into Pictures/Screenshots.",
        schedule: "0 */30 * * * *",
        code: include_str!("../../examples/tidy_screenshots.lua"),
    },
    Example {
        slug: "backup-notes",
        title: "Back up notes",
        description: "Copy Markdown notes to a backup folder, skipping files already copied.",
        schedule: "0 0 18 * * *",
        code: include_str!("../../examples/backup_notes.lua"),
    },
];

pub fn find_example(slug: &str) -> Option<&'static Example> {
    EXAMPLES.iter().find(|e| e.slug == slug)
}
