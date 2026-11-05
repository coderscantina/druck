//! Druck desktop: a native writing app and book setter on the same engine as the CLI.

mod a11y;
mod agent;
mod app;
mod assets;
mod editor;
#[cfg(any(debug_assertions, feature = "measure"))]
mod gate;
/// A shipped build has no hidden behaviour: these are all that is left of [`gate`].
#[cfg(not(any(debug_assertions, feature = "measure")))]
mod gate {
    pub const fn measuring() -> bool {
        false
    }

    pub fn at_launch(_: &mut gpui::App) {}

    pub fn apply(
        _: &crate::workspace::Parts,
        _: gpui::Entity<crate::workspace::Workspace>,
        _: &mut gpui::Window,
        _: &mut gpui::Context<crate::workspace::Workspace>,
    ) {
    }
}
mod inspector;
mod launch;
mod open_urls;
mod palette;
mod persist;
mod preview;
mod session;
mod settings;
mod shell;
mod sidebar;
mod source;
mod spell;
mod theme_file;
mod tokens;
mod versions;
mod workspace;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;

use session::Session;
use tokens::Scheme;

#[derive(Parser)]
#[command(version = druck::editor::VERSION)]
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
    // The CLI starts this executable again as the tool that leaves margin notes. See `agent::note_tool`.
    if std::env::args().nth(1).as_deref() == Some(agent::NOTE_TOOL_FLAG) {
        let served = std::env::current_dir().and_then(|folder| agent::note_tool::serve(&folder));
        return if served.is_ok() {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        };
    }
    let cli = Cli::parse();
    if let Some(path) = cli.open.as_deref().filter(|path| !path.exists()) {
        eprintln!("cannot open {}: no such file or folder", path.display());
        return ExitCode::FAILURE;
    }
    // Anything else that keeps a document from opening shows on the launch screen.
    let (session, error) = match cli.open.as_deref().map(|path| (path, Session::open(path))) {
        Some((_, Ok(session))) => (Some(session), None),
        Some((path, Err(problem))) => (None, Some((problem, path.to_owned()))),
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
    // Documents the system hands over, which can arrive before or after the window exists.
    let (opened, documents) = futures::channel::mpsc::unbounded();
    let application = gpui_platform::application().with_assets(assets::Assets);
    application.on_open_urls(move |urls| {
        opened.unbounded_send(open_urls::paths(&urls)).ok();
    });
    application.run(move |cx| app::open(session, error, theme, cli.appearance, documents, cx));
    ExitCode::SUCCESS
}
