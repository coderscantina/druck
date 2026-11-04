//! Druck desktop: a native composer and theme editor on the same engine as the CLI.

mod agent;
mod app;
mod assets;
mod editor;
mod gate;
mod inspector;
mod launch;
mod persist;
mod preview;
mod session;
mod sidebar;
mod source;
mod theme_file;
mod tokens;
mod workspace;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;

use session::Session;
use tokens::Scheme;

#[derive(Parser)]
#[command(version)]
struct Cli {
    /// Markdown file or project folder to open.
    #[arg(long, value_name = "PATH")]
    open: Option<PathBuf>,
    /// Theme to use instead of the document's: a JSON file, or the name of a common theme.
    #[arg(long, value_name = "THEME")]
    theme: Option<PathBuf>,
    /// Use this appearance instead of following the system.
    #[arg(long, value_enum)]
    appearance: Option<Scheme>,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    if let Some(path) = cli.open.as_deref().filter(|path| !path.exists()) {
        eprintln!("cannot open {}: no such file or folder", path.display());
        return ExitCode::FAILURE;
    }
    // Anything else that keeps a document from opening shows on the launch screen.
    let (session, error) = match cli.open.as_deref().map(Session::open) {
        Some(Ok(session)) => (Some(session), None),
        Some(Err(problem)) => (None, Some(problem)),
        None => (None, None),
    };
    let theme = cli.theme.as_deref().map(|theme| {
        let working_dir = std::env::current_dir().unwrap_or_default();
        let dirs = session.as_ref().map_or_else(
            || druck::input::ThemeDirs {
                project: None,
                shared: druck::resource::FileResolver::shared_themes(),
            },
            Session::theme_dirs,
        );
        druck::editor::engine::theme_file(&druck::resource::FileResolver::new(), theme, &working_dir, &dirs)
    });
    gpui_platform::application()
        .with_assets(assets::Assets)
        .run(move |cx| app::open(session, error, theme, cli.appearance, cx));
    ExitCode::SUCCESS
}
