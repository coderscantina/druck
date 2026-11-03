//! Druck desktop: a native composer and theme editor on the same engine as the CLI.

mod agent;
mod app;
mod assets;
mod document;
mod editor;
mod inspector;
mod preview;
mod sidebar;
mod source;
mod theme_file;
mod tokens;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;

use document::Opened;
use tokens::Scheme;

#[derive(Parser)]
#[command(version)]
struct Cli {
    /// Markdown document to open.
    #[arg(long, value_name = "PATH")]
    open: Option<PathBuf>,
    /// Theme the Styles tab edits: a JSON file, or the name of a common theme.
    #[arg(long, value_name = "THEME")]
    theme: Option<PathBuf>,
    /// Use this appearance instead of following the system.
    #[arg(long, value_enum)]
    appearance: Option<Scheme>,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let opened = match cli.open.as_deref().map(Opened::read).transpose() {
        Ok(opened) => opened,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::FAILURE;
        }
    };
    let theme = cli.theme.as_deref().map(|theme| {
        let working_dir = std::env::current_dir().unwrap_or_default();
        druck::theme_path::resolve(theme, &working_dir)
    });
    gpui_platform::application()
        .with_assets(assets::Assets)
        .run(move |cx| app::open(opened, theme, cli.appearance, cx));
    ExitCode::SUCCESS
}
