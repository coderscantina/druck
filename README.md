# Kyber

A native Rust Markdown-to-PDF renderer for carefully typeset articles, reports, and short manuscripts.

The CLI renders complete documents to PDF: paragraph-wide line breaking with English and German hyphenation, scored page breaks, footnotes, one and two columns, images, tables, title pages, contents, cross-references, and citations from BibTeX. Release acceptance (milestone 10) is still open.

Kyber's own license is not chosen yet, so do not distribute builds. Third-party licenses are listed in [the notices](THIRD_PARTY_NOTICES.md).

```sh
cargo run -- render samples/en.md -o en.pdf
cargo run -- check report.md --theme my-theme.json --set toc=true --print-config
```

- [Authoring](docs/AUTHORING.md): front matter, CLI overrides, and layout directives.
- [Themes](docs/THEMES.md): the JSON theme format, with [its schema](schema/theme.v1.schema.json) and [the default theme](themes/default.json).
- [Phase 1 product briefing](docs/BRIEFING.md): CLI renderer and JSON templates/themes.
- [Phase 1 milestones](docs/PHASE_1_MILESTONES.md): implementation sequence and completion gates.
- [Progress](docs/PROGRESS.md): current state and next action.
- [Decisions](docs/DECISIONS.md): settled implementation choices.
- [Release](docs/RELEASE.md): building, checking, and packaging a release.
- [Phase 2 public crate/API](docs/RUST_CRATE_API_PHASE_2.md): native client integration after phase 1.

## Starting an implementation session

Ask the agent to implement a named milestone or a bounded slice of it. For example:

> Implement the configuration and resource-origin slice of milestone 01. Follow AGENTS.md, verify the changed behavior, and update progress. Do not commit.

For a resumed task, point to the current milestone and progress note. The agent should establish the actual code state before continuing. Acceptance checks and unresolved work belong in the repository, not only in conversation history.
