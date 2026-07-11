pub mod api;
pub mod engine;
pub mod sandbox;
pub mod system;

use serde::Serialize;

/// A ready-to-use automation offered on the "New automation" page.
#[derive(Debug, Clone, Serialize)]
pub struct Example {
    pub slug: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub schedule: &'static str,
    pub code: &'static str,
    pub run_on_startup: bool,
    /// Folder to watch ("" for none).
    pub watch_path: &'static str,
    pub watch_pattern: &'static str,
}

/// Defaults for the trigger fields, so each template only lists what it uses.
const NO_TRIGGERS: (bool, &str, &str) = (false, "", "");

pub const EXAMPLES: &[Example] = &[
    Example {
        slug: "hello-world",
        title: "Hello world",
        description: "The smallest possible automation. A good place to start.",
        schedule: "",
        code: include_str!("../../scripts/hello_world.lua"),
        run_on_startup: NO_TRIGGERS.0,
        watch_path: NO_TRIGGERS.1,
        watch_pattern: NO_TRIGGERS.2,
    },
    Example {
        slug: "organize-pdfs",
        title: "Organize PDF files",
        description: "Move PDFs from Downloads into Documents/PDF.",
        schedule: "0 0 * * * *",
        code: include_str!("../../scripts/organize_pdfs.lua"),
        run_on_startup: NO_TRIGGERS.0,
        watch_path: NO_TRIGGERS.1,
        watch_pattern: NO_TRIGGERS.2,
    },
    Example {
        slug: "tidy-screenshots",
        title: "Tidy screenshots",
        description: "Move screenshots off the Desktop into Pictures/Screenshots.",
        schedule: "0 */30 * * * *",
        code: include_str!("../../scripts/tidy_screenshots.lua"),
        run_on_startup: NO_TRIGGERS.0,
        watch_path: NO_TRIGGERS.1,
        watch_pattern: NO_TRIGGERS.2,
    },
    Example {
        slug: "backup-notes",
        title: "Back up notes",
        description: "Copy Markdown notes to a backup folder, skipping files already copied.",
        schedule: "0 0 18 * * *",
        code: include_str!("../../scripts/backup_notes.lua"),
        run_on_startup: NO_TRIGGERS.0,
        watch_path: NO_TRIGGERS.1,
        watch_pattern: NO_TRIGGERS.2,
    },
    Example {
        slug: "open-work-apps",
        title: "Open my work apps",
        description: "Open the apps you use every day in one click, or automatically at sign-in.",
        schedule: "",
        code: include_str!("../../scripts/open_work_apps.lua"),
        run_on_startup: true,
        watch_path: "",
        watch_pattern: "",
    },
    Example {
        slug: "list-apps",
        title: "List my apps",
        description: "Show the app names app.open() understands, and what's running now.",
        schedule: "",
        code: include_str!("../../scripts/list_apps.lua"),
        run_on_startup: NO_TRIGGERS.0,
        watch_path: NO_TRIGGERS.1,
        watch_pattern: NO_TRIGGERS.2,
    },
    Example {
        slug: "file-new-pdfs",
        title: "File new PDFs",
        description: "The moment a PDF is downloaded, move it into a Documents/PDF folder for this month.",
        schedule: "",
        code: include_str!("../../scripts/sort_new_pdfs.lua"),
        run_on_startup: false,
        watch_path: "~/Downloads",
        watch_pattern: "*.pdf",
    },
    Example {
        slug: "old-installers",
        title: "Tidy old installers",
        description: "Move installers older than 30 days out of Downloads.",
        schedule: "0 0 10 * * Mon",
        code: include_str!("../../scripts/old_installers.lua"),
        run_on_startup: NO_TRIGGERS.0,
        watch_path: NO_TRIGGERS.1,
        watch_pattern: NO_TRIGGERS.2,
    },
];

pub fn find_example(slug: &str) -> Option<&'static Example> {
    EXAMPLES.iter().find(|e| e.slug == slug)
}
