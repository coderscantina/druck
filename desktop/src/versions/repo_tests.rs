//! Every test here runs against a temporary real repo and compares with what the `git` CLI reports.

use std::process::Command;

use super::super::git_cli::{Merged, Refused};
use super::*;

struct Fixture {
    root: PathBuf,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).ok();
    }
}

fn fixture(name: &str) -> Fixture {
    let root = std::env::temp_dir().join(format!("druck-repo-{name}-{}", std::process::id()));
    fs::remove_dir_all(&root).ok();
    fs::create_dir_all(&root).unwrap();
    Fixture {
        root: fs::canonicalize(root).unwrap(),
    }
}

fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git").args(args).current_dir(dir).output().unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim_end().to_owned()
}

/// A repo with one commit on `main`, and settings of its own whatever the machine's are.
fn repo_at(dir: &Path) -> Repo {
    fs::create_dir_all(dir).unwrap();
    git(dir, &["init", "-q", "-b", "main"]);
    for (key, value) in [
        ("user.name", "Test Author"),
        ("user.email", "author@example.com"),
        ("commit.gpgsign", "false"),
    ] {
        git(dir, &["config", key, value]);
    }
    fs::create_dir_all(dir.join("manuscript")).unwrap();
    fs::write(dir.join("manuscript/one.md"), "# One\n\nFirst words here.\n").unwrap();
    git(dir, &["add", "."]);
    git(dir, &["commit", "-q", "-m", "First"]);
    Repo::open(dir).unwrap().unwrap()
}

fn clean(dir: &Path) -> bool {
    git(dir, &["status", "--porcelain"]).is_empty()
}

#[test]
fn a_commit_reads_correctly_in_plain_git() {
    let fixture = fixture("commit");
    let dir = fixture.root.join("book");
    let mut repo = repo_at(&dir);
    fs::write(dir.join("manuscript/one.md"), "# One\n\nFirst words here, and more.\n").unwrap();
    fs::write(dir.join("manuscript/two.md"), "# Two\n\nGröße.\n").unwrap();
    let id = repo.commit("Add the second chapter\n\nWith a body.", None).unwrap();
    assert_eq!(git(&dir, &["rev-parse", "HEAD"]), id);
    assert_eq!(
        git(&dir, &["log", "-1", "--format=%s|%an|%ae"]),
        "Add the second chapter|Test Author|author@example.com"
    );
    assert_eq!(git(&dir, &["log", "-1", "--format=%b"]), "With a body.");
    assert_eq!(git(&dir, &["rev-list", "--count", "HEAD"]), "2");
    assert_eq!(git(&dir, &["show", "HEAD:manuscript/two.md"]), "# Two\n\nGröße.");
    assert!(clean(&dir), "{}", git(&dir, &["status", "--porcelain"]));
    assert_eq!(
        git(&dir, &["reflog", "-1", "--format=%gs"]),
        "commit: Add the second chapter"
    );
    git(&dir, &["fsck", "--strict"]);
}

#[test]
fn deleting_a_folders_last_file_leaves_no_empty_tree() {
    let fixture = fixture("delete");
    let dir = fixture.root.join("book");
    let mut repo = repo_at(&dir);
    fs::write(dir.join("keep.md"), "stays\n").unwrap();
    git(&dir, &["add", "."]);
    git(&dir, &["commit", "-qm", "Second"]);
    fs::remove_file(dir.join("manuscript/one.md")).unwrap();
    fs::remove_dir(dir.join("manuscript")).unwrap();
    repo.commit("Remove the only chapter", None).unwrap();
    assert_eq!(git(&dir, &["ls-tree", "--name-only", "HEAD"]), "keep.md");
    assert!(clean(&dir), "{}", git(&dir, &["status", "--porcelain"]));
    git(&dir, &["fsck", "--strict"]);
}

#[test]
fn nothing_is_committed_unless_asked_and_nothing_empty_at_all() {
    let fixture = fixture("empty");
    let dir = fixture.root.join("book");
    let mut repo = repo_at(&dir);
    fs::write(dir.join("manuscript/one.md"), "changed\n").unwrap();
    let _ = repo.overview(10).unwrap();
    assert_eq!(git(&dir, &["rev-list", "--count", "HEAD"]), "1");
    fs::write(dir.join("manuscript/one.md"), "# One\n\nFirst words here.\n").unwrap();
    assert_eq!(repo.commit("Nothing", None), Err(Problem::NothingToCommit));
}

#[test]
fn a_new_repo_takes_its_first_commit() {
    let fixture = fixture("init");
    let dir = fixture.root.join("book");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("a.md"), "words\n").unwrap();
    Repo::init(&dir).unwrap();
    git(&dir, &["config", "user.name", "Test Author"]);
    git(&dir, &["config", "user.email", "author@example.com"]);
    git(&dir, &["config", "commit.gpgsign", "false"]);
    let mut repo_again = Repo::open(&dir).unwrap().unwrap();
    assert_eq!(repo_again.changes().unwrap()[0].kind, ChangeKind::Added);
    repo_again.commit("Start", None).unwrap();
    assert!(clean(&dir));
    assert_eq!(git(&dir, &["rev-list", "--count", "HEAD"]), "1");
    assert_eq!(repo_again.overview(10).unwrap().log[0].summary, "Start");
    // A folder without one is not a repo until asked.
    let other = fixture.root.join("other");
    fs::create_dir_all(&other).unwrap();
    assert!(Repo::open(&other).unwrap().is_none());
}

#[test]
fn identity_set_here_is_what_git_reads() {
    let fixture = fixture("identity");
    let dir = fixture.root.join("book");
    let repo = repo_at(&dir);
    repo.set_identity("Ada \"A\" Lovelace", "ada@example.com").unwrap();
    assert_eq!(
        git(&dir, &["config", "--local", "--get-all", "user.name"])
            .lines()
            .last(),
        Some("Ada \"A\" Lovelace")
    );
    assert_eq!(
        git(&dir, &["config", "--local", "--get-all", "user.email"])
            .lines()
            .last(),
        Some("ada@example.com")
    );
}

#[test]
fn changes_match_git_status_and_count_words() {
    let fixture = fixture("status");
    let dir = fixture.root.join("book");
    repo_at(&dir);
    fs::write(dir.join(".gitignore"), "build/\n").unwrap();
    fs::create_dir_all(dir.join("build")).unwrap();
    fs::write(dir.join("build/out.pdf"), "ignored").unwrap();
    fs::write(dir.join("manuscript/one.md"), "# One\n\nFirst new words here.\n").unwrap();
    fs::write(dir.join("manuscript/two.md"), "# Two\n").unwrap();
    let changes = Repo::open(&dir).unwrap().unwrap().changes().unwrap();
    let names: Vec<&str> = changes.iter().map(|change| change.name.as_str()).collect();
    assert_eq!(names, [".gitignore", "manuscript/one.md", "manuscript/two.md"]);
    assert_eq!(changes[1].kind, ChangeKind::Modified);
    assert_eq!(changes[1].words, Some((1, 0)));
    assert_eq!(changes[2].kind, ChangeKind::Added);
    let porcelain = git(&dir, &["status", "--porcelain", "-uall"]);
    assert_eq!(porcelain.lines().count(), changes.len());
    fs::remove_file(dir.join("manuscript/one.md")).unwrap();
    let changes = Repo::open(&dir).unwrap().unwrap().changes().unwrap();
    assert_eq!(changes[1].kind, ChangeKind::Removed);
    assert_eq!(changes[1].words, Some((0, 4)));
}

#[test]
fn a_moved_file_shows_once_as_moved_and_commits_as_a_rename() {
    let fixture = fixture("moved");
    let dir = fixture.root.join("book");
    repo_at(&dir);
    fs::create_dir_all(dir.join("archiv")).unwrap();
    fs::rename(dir.join("manuscript/one.md"), dir.join("archiv/one.md")).unwrap();
    let mut repo = Repo::open(&dir).unwrap().unwrap();
    let overview = repo.overview(5).unwrap();
    let listed: Vec<_> = overview.listed().collect();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].kind, ChangeKind::Moved);
    assert_eq!(listed[0].from.as_deref(), Some("manuscript/one.md"));
    assert_eq!(listed[0].words, Some((0, 0)));
    repo.commit("Archive", None).unwrap();
    assert_eq!(
        git(&dir, &["show", "--name-status", "--format=", "-M", "HEAD"]),
        "R100\tmanuscript/one.md\tarchiv/one.md"
    );
    assert!(clean(&dir));
}

#[test]
fn a_move_another_tool_staged_shows_once_as_moved() {
    let fixture = fixture("staged-move");
    let dir = fixture.root.join("book");
    repo_at(&dir);
    git(&dir, &["mv", "manuscript/one.md", "manuscript/01-one.md"]);
    let mut repo = Repo::open(&dir).unwrap().unwrap();
    let overview = repo.overview(5).unwrap();
    let listed: Vec<_> = overview.listed().collect();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].kind, ChangeKind::Moved);
    assert_eq!(listed[0].from.as_deref(), Some("manuscript/one.md"));
    assert_eq!(listed[0].words, Some((0, 0)));
    repo.commit("Number the chapter", None).unwrap();
    assert_eq!(
        git(&dir, &["show", "--name-status", "--format=", "-M", "HEAD"]),
        "R100\tmanuscript/one.md\tmanuscript/01-one.md"
    );
    assert!(clean(&dir));
}

#[test]
fn a_book_in_a_bigger_repo_commits_only_its_folder() {
    let fixture = fixture("scope");
    let dir = fixture.root.join("shelf");
    let _ = repo_at(&dir);
    let book = dir.join("book-one");
    fs::create_dir_all(book.join("manuscript")).unwrap();
    fs::write(book.join("manuscript/a.md"), "words\n").unwrap();
    fs::create_dir_all(dir.join("book-two")).unwrap();
    fs::write(dir.join("book-two/b.md"), "other book\n").unwrap();
    fs::write(dir.join("staged.md"), "staged elsewhere\n").unwrap();
    git(&dir, &["add", "staged.md"]);
    let mut repo = Repo::open(&book).unwrap().unwrap();
    let changes = repo.changes().unwrap();
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].name, "manuscript/a.md");
    assert_eq!(changes[0].path, book.join("manuscript/a.md"));
    repo.commit("Book one", None).unwrap();
    assert_eq!(
        git(&dir, &["show", "--name-only", "--format=", "HEAD"]),
        "book-one/manuscript/a.md"
    );
    let status = git(&dir, &["status", "--porcelain", "-uall"]);
    assert_eq!(status, "A  staged.md\n?? book-two/b.md");
}

#[test]
fn history_matches_git_log() {
    let fixture = fixture("log");
    let dir = fixture.root.join("book");
    let repo = repo_at(&dir);
    for (at, words) in ["second", "third", "fourth"].iter().enumerate() {
        fs::write(dir.join("manuscript/one.md"), format!("{words}\n")).unwrap();
        fs::write(dir.join("other.md"), format!("{at}\n")).unwrap();
        git(&dir, &["add", "."]);
        git(&dir, &["commit", "-q", "-m", words]);
        fs::write(dir.join("other.md"), format!("{at} again\n")).unwrap();
        git(&dir, &["add", "."]);
        git(&dir, &["commit", "-q", "-m", "elsewhere"]);
    }
    let log = repo.log(100).unwrap();
    let ours: Vec<String> = log.iter().map(|commit| commit.id.clone()).collect();
    let theirs: Vec<String> = git(&dir, &["log", "--format=%H"]).lines().map(str::to_owned).collect();
    assert_eq!(ours.len(), theirs.len());
    let (mut sorted_ours, mut sorted_theirs) = (ours.clone(), theirs.clone());
    sorted_ours.sort();
    sorted_theirs.sort();
    assert_eq!(sorted_ours, sorted_theirs);
    let file = repo.file_history(&dir.join("manuscript/one.md")).unwrap();
    let cli: Vec<String> = git(&dir, &["log", "--reverse", "--format=%H", "--", "manuscript/one.md"])
        .lines()
        .map(str::to_owned)
        .collect();
    assert_eq!(
        file.iter().map(|(commit, _)| commit.id.clone()).collect::<Vec<_>>(),
        cli
    );
    assert_eq!(file[1].1, super::super::history::blob_id(b"second\n"));
    assert_eq!(
        repo.text_at(&file[1].0.id, &dir.join("manuscript/one.md"))
            .unwrap()
            .as_deref(),
        Some("second\n")
    );
}

#[test]
fn branches_made_and_deleted_here_are_what_git_lists() {
    let fixture = fixture("branches");
    let dir = fixture.root.join("book");
    let repo = repo_at(&dir);
    repo.create_branch("second-ending").unwrap();
    assert_eq!(
        repo.create_branch("second-ending"),
        Err(Problem::BranchExists("second-ending".to_owned()))
    );
    assert!(matches!(repo.create_branch("no spaces"), Err(Problem::BadName(_))));
    assert_eq!(
        git(&dir, &["branch", "--format=%(refname:short)"]),
        "main\nsecond-ending"
    );
    let branches = repo.branches().unwrap();
    assert_eq!(branches.len(), 2);
    assert!(branches[0].merged && branches[1].current);
    assert_eq!(git(&dir, &["branch", "--show-current"]), "second-ending");
    assert_eq!(git(&dir, &["status", "--porcelain"]), "");
    git(&dir, &["switch", "-q", "main"]);
    let mut repo = Repo::open(&dir).unwrap().unwrap();
    assert!(repo.delete_branch("main").is_err());
    git(
        &dir,
        &["config", "branch.second-ending.description", "kept until deleted"],
    );
    repo.delete_branch("second-ending").unwrap();
    assert_eq!(git(&dir, &["branch", "--format=%(refname:short)"]), "main");
    assert!(
        !fs::read_to_string(dir.join(".git/config"))
            .unwrap()
            .contains("second-ending")
    );
}

#[test]
fn switching_keeps_what_does_not_overlap_and_refuses_what_does() {
    let Some(cli) = Git::find() else {
        return;
    };
    let fixture = fixture("switch");
    let dir = fixture.root.join("book");
    let repo = repo_at(&dir);
    git(&dir, &["switch", "-q", "-c", "other"]);
    fs::write(dir.join("manuscript/one.md"), "# One\n\nOn the other branch.\n").unwrap();
    git(&dir, &["commit", "-qam", "Other"]);
    git(&dir, &["switch", "-q", "main"]);
    fs::write(dir.join("loose.md"), "not committed\n").unwrap();
    cli.switch(&dir, "other").unwrap();
    assert_eq!(git(&dir, &["branch", "--show-current"]), "other");
    assert_eq!(fs::read_to_string(dir.join("loose.md")).unwrap(), "not committed\n");
    assert_eq!(repo.overview(10).unwrap().branch, "other");
    fs::write(dir.join("manuscript/one.md"), "typed but not committed\n").unwrap();
    assert_eq!(cli.switch(&dir, "main"), Err(Refused::LocalChanges));
    assert_eq!(git(&dir, &["branch", "--show-current"]), "other");
    assert_eq!(
        fs::read_to_string(dir.join("manuscript/one.md")).unwrap(),
        "typed but not committed\n"
    );
}

#[test]
fn a_clean_merge_is_the_same_as_gits() {
    let Some(cli) = Git::find() else {
        return;
    };
    let fixture = fixture("merge");
    let dir = fixture.root.join("book");
    let repo = repo_at(&dir);
    git(&dir, &["switch", "-q", "-c", "other"]);
    fs::write(dir.join("manuscript/two.md"), "from other\n").unwrap();
    git(&dir, &["add", "."]);
    git(&dir, &["commit", "-qm", "Other"]);
    git(&dir, &["switch", "-q", "main"]);
    fs::write(dir.join("manuscript/three.md"), "from main\n").unwrap();
    git(&dir, &["add", "."]);
    git(&dir, &["commit", "-qm", "Main"]);
    assert_eq!(cli.merge(&dir, repo.git_dir(), "other"), Ok(Merged::Done));
    assert_eq!(git(&dir, &["log", "-1", "--format=%P"]).split(' ').count(), 2);
    assert!(clean(&dir));
    assert!(dir.join("manuscript/two.md").exists());
    let branches = repo.branches().unwrap();
    assert!(branches.iter().find(|branch| branch.name == "other").unwrap().merged);
}

#[test]
fn an_overlapping_merge_gives_each_side_and_a_commit_here_finishes_it() {
    let Some(cli) = Git::find() else {
        return;
    };
    let fixture = fixture("conflict");
    let dir = fixture.root.join("book");
    let _ = repo_at(&dir);
    git(&dir, &["switch", "-q", "-c", "other"]);
    fs::write(dir.join("manuscript/one.md"), "# One\n\nTheir words.\n").unwrap();
    fs::write(dir.join("clean.md"), "merged without trouble\n").unwrap();
    git(&dir, &["add", "."]);
    git(&dir, &["commit", "-qm", "Other"]);
    git(&dir, &["switch", "-q", "main"]);
    fs::write(dir.join("manuscript/one.md"), "# One\n\nMy words.\n").unwrap();
    git(&dir, &["commit", "-qam", "Mine"]);
    let main_tip = git(&dir, &["rev-parse", "HEAD"]);
    let other_tip = git(&dir, &["rev-parse", "other"]);

    let mut repo = Repo::open(&dir).unwrap().unwrap();
    assert_eq!(cli.merge(&dir, repo.git_dir(), "other"), Ok(Merged::Conflicts));
    let repo_after = Repo::open(&dir).unwrap().unwrap();
    let conflicts = repo_after.conflicts().unwrap();
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].name, "manuscript/one.md");
    assert_eq!(conflicts[0].base.as_deref(), Some("# One\n\nFirst words here.\n"));
    assert_eq!(conflicts[0].theirs.as_deref(), Some("# One\n\nTheir words.\n"));
    assert!(conflicts[0].in_book && conflicts[0].ours_exists);
    assert_eq!(
        repo_after.overview(5).unwrap().merging.as_deref(),
        Some("Merge branch 'other'")
    );
    assert_eq!(
        git(&dir, &["diff", "--name-only", "--diff-filter=U"]),
        "manuscript/one.md"
    );

    // What the conflict line leaves in the file once the author chose.
    fs::write(dir.join("manuscript/one.md"), "# One\n\nBoth our words.\n").unwrap();
    repo = Repo::open(&dir).unwrap().unwrap();
    assert_eq!(
        repo.changes()
            .unwrap()
            .iter()
            .find(|change| change.name.ends_with("one.md"))
            .unwrap()
            .kind,
        ChangeKind::Unmerged
    );
    repo.commit("Merge branch 'other'", None).unwrap();
    let parents = git(&dir, &["log", "-1", "--format=%P"]);
    assert_eq!(parents, format!("{main_tip} {other_tip}"));
    assert!(clean(&dir), "{}", git(&dir, &["status", "--porcelain"]));
    assert_eq!(
        git(&dir, &["show", "HEAD:manuscript/one.md"]),
        "# One\n\nBoth our words."
    );
    assert_eq!(git(&dir, &["show", "HEAD:clean.md"]), "merged without trouble");
    assert!(!dir.join(".git/MERGE_HEAD").exists());
    git(&dir, &["fsck", "--strict"]);
}

#[test]
fn aborting_a_merge_leaves_things_as_they_were() {
    let Some(cli) = Git::find() else {
        return;
    };
    let fixture = fixture("abort");
    let dir = fixture.root.join("book");
    let repo = repo_at(&dir);
    git(&dir, &["switch", "-q", "-c", "other"]);
    fs::write(dir.join("manuscript/one.md"), "theirs\n").unwrap();
    git(&dir, &["commit", "-qam", "Other"]);
    git(&dir, &["switch", "-q", "main"]);
    fs::write(dir.join("manuscript/one.md"), "mine\n").unwrap();
    git(&dir, &["commit", "-qam", "Mine"]);
    assert_eq!(cli.merge(&dir, repo.git_dir(), "other"), Ok(Merged::Conflicts));
    cli.abort_merge(&dir).unwrap();
    assert!(clean(&dir));
    assert_eq!(fs::read_to_string(dir.join("manuscript/one.md")).unwrap(), "mine\n");
}

#[test]
fn push_and_pull_against_a_local_bare_repo_track_sync_state() {
    let Some(cli) = Git::find() else {
        return;
    };
    let fixture = fixture("remote");
    let bare = fixture.root.join("remote.git");
    git(&fixture.root, &["init", "-q", "--bare", "-b", "main", "remote.git"]);
    let dir = fixture.root.join("book");
    let repo = repo_at(&dir);
    assert_eq!(repo.sync().unwrap().0, Sync::NoRemote);
    git(&dir, &["remote", "add", "origin", bare.to_str().unwrap()]);
    let mut repo_again = Repo::open(&dir).unwrap().unwrap();
    assert_eq!(repo_again.sync().unwrap(), (Sync::NotPushed, Some("origin".to_owned())));

    cli.push(&dir, Some(("origin", "main"))).unwrap();
    repo_again = Repo::open(&dir).unwrap().unwrap();
    assert_eq!(repo_again.sync().unwrap().0, Sync::Synced);
    assert_eq!(git(&bare, &["rev-parse", "main"]), git(&dir, &["rev-parse", "HEAD"]));

    fs::write(dir.join("manuscript/one.md"), "more\n").unwrap();
    repo_again.commit("Local", None).unwrap();
    assert_eq!(repo_again.sync().unwrap().0, Sync::Ahead(1));
    cli.push(&dir, None).unwrap();
    assert_eq!(Repo::open(&dir).unwrap().unwrap().sync().unwrap().0, Sync::Synced);

    let elsewhere = fixture.root.join("elsewhere");
    git(&fixture.root, &["clone", "-q", bare.to_str().unwrap(), "elsewhere"]);
    git(&elsewhere, &["config", "user.name", "Other"]);
    git(&elsewhere, &["config", "user.email", "other@example.com"]);
    git(&elsewhere, &["config", "commit.gpgsign", "false"]);
    fs::write(elsewhere.join("new.md"), "from elsewhere\n").unwrap();
    git(&elsewhere, &["add", "."]);
    git(&elsewhere, &["commit", "-qm", "Elsewhere"]);
    git(&elsewhere, &["push", "-q"]);

    cli.fetch(&dir).unwrap();
    let after_fetch = Repo::open(&dir).unwrap().unwrap();
    assert_eq!(after_fetch.sync().unwrap().0, Sync::Behind(1));
    let upstream = after_fetch.upstream().unwrap().unwrap();
    assert_eq!(upstream, "origin/main");
    assert_eq!(cli.merge(&dir, after_fetch.git_dir(), &upstream), Ok(Merged::Done));
    assert_eq!(Repo::open(&dir).unwrap().unwrap().sync().unwrap().0, Sync::Synced);
    assert!(dir.join("new.md").exists());
}

#[test]
fn a_branch_name_or_remote_failure_comes_back_in_words() {
    let Some(cli) = Git::find() else {
        return;
    };
    let fixture = fixture("fail");
    let dir = fixture.root.join("book");
    let repo = repo_at(&dir);
    assert!(matches!(
        cli.merge(&dir, repo.git_dir(), "no-such-branch"),
        Err(Refused::Said(_))
    ));
    assert!(cli.push(&dir, Some(("nowhere", "main"))).is_err());
}

#[test]
fn folders_of_sync_services_are_named() {
    assert_eq!(
        cloud_service(Path::new("/Users/a/Library/Mobile Documents/com~apple~CloudDocs/book")),
        Some("iCloud Drive")
    );
    assert_eq!(cloud_service(Path::new("/Users/a/Dropbox/book")), Some("Dropbox"));
    assert_eq!(
        cloud_service(Path::new("/Users/a/Library/CloudStorage/Dropbox-Work/book")),
        Some("Dropbox")
    );
    assert_eq!(
        cloud_service(Path::new("/Users/a/Library/CloudStorage/GoogleDrive-a@b/book")),
        Some("Google Drive")
    );
    assert_eq!(cloud_service(Path::new("/Users/a/Books/book")), None);
}

#[test]
fn a_new_repo_leaves_exports_out_and_keeps_an_ignore_file_it_finds() {
    let fresh = std::env::temp_dir().join(format!("druck-ignore-{}", std::process::id()));
    fs::remove_dir_all(&fresh).ok();
    fs::create_dir_all(fresh.join("build")).expect("folder");
    fs::write(fresh.join("a.md"), "A\n").expect("file");
    fs::write(fresh.join("build/book.pdf"), "pdf").expect("file");
    let repo = Repo::init(&fresh).expect("a repo");
    assert_eq!(
        fs::read_to_string(fresh.join(".gitignore")).expect("written"),
        "build/\n"
    );
    let changed: Vec<_> = (repo.changes().expect("changes").into_iter())
        .map(|change| change.path)
        .collect();
    assert!(
        changed.iter().all(|path| !path.to_string_lossy().contains("build")),
        "{changed:?}"
    );
    fs::remove_dir_all(&fresh).ok();

    fs::create_dir_all(&fresh).expect("folder");
    fs::write(fresh.join(".gitignore"), "mine/\n").expect("file");
    Repo::init(&fresh).expect("a repo");
    assert_eq!(fs::read_to_string(fresh.join(".gitignore")).expect("kept"), "mine/\n");
    fs::remove_dir_all(&fresh).ok();
}

/// What once happened in the real book: Git reports "both modified", and the app must not settle that by merging words
/// into a sentence neither side wrote.
#[test]
fn a_hunk_git_calls_conflicting_goes_to_a_person_and_never_merges_by_words() {
    use crate::persist::merge::Choice;
    use crate::persist::sync::{ChangeKind, FileSync};
    use crate::session::Session;

    let fixture = fixture("word-merge");
    let dir = fixture.root.join("book");
    let _ = repo_at(&dir);
    let chapter = dir.join("manuscript/one.md");
    let manifest = "title: T\nchapters:\n  - manuscript/one.md\n";
    fs::write(dir.join("druck.yaml"), manifest).unwrap();
    let write = |sentence: &str| fs::write(&chapter, format!("# One\n\nDa war {sentence}.\n\nDas Ende.\n")).unwrap();
    let commit = |message: &str| {
        git(&dir, &["add", "."]);
        git(&dir, &["commit", "-q", "-m", message]);
    };
    write("nichts");
    commit("Base");
    git(&dir, &["checkout", "-q", "-b", "probe"]);
    write("gar nichts");
    commit("Theirs");
    git(&dir, &["checkout", "-q", "main"]);
    write("nur Leere");
    commit("Ours");

    let mut session = Session::open(&dir).unwrap();
    let cli = Git::find().expect("these tests need git");
    let repo = Repo::open(&dir).unwrap().unwrap();
    assert!(matches!(
        cli.merge(&dir, repo.git_dir(), "probe"),
        Ok(Merged::Conflicts)
    ));
    assert!(git(&dir, &["status", "--porcelain"]).contains("UU manuscript/one.md"));
    let conflict = repo.conflicts().unwrap().remove(0);
    let (base, theirs) = (conflict.base.unwrap(), conflict.theirs.unwrap());
    fs::write(&chapter, &theirs).unwrap();

    let mut sync = FileSync::new(None);
    let change = sync.take_git_conflict(&mut session, &chapter, &base, &theirs).unwrap();
    assert_eq!(change.kind, ChangeKind::Conflict);
    assert_eq!(sync.conflicts().collect::<Vec<_>>(), [chapter.as_path()]);
    let mine = "# One\n\nDa war nur Leere.\n\nDas Ende.\n";
    assert_eq!(session.file_text(&chapter).as_deref(), Some(mine));

    // A later look at the same file, as after any change in the folder, leaves it to the person.
    assert!(
        sync.on_disk(&mut session, &chapter, Some(theirs.clone().into_bytes()))
            .is_none()
    );
    assert!(!sync.can_settle_a_conflict(&session));
    let passage = sync.conflict_passage(&session, &chapter).unwrap();
    assert_eq!(passage.mine, "Da war nur Leere.\n");
    assert_eq!(passage.theirs, "Da war gar nichts.\n");

    sync.resolve(&mut session, &chapter, Choice::Theirs).unwrap();
    assert_eq!(session.file_text(&chapter).as_deref(), Some(theirs.as_str()));
    assert_eq!(sync.conflicts().count(), 0);
}

/// Git calls lines next to each other a conflict too, and what it merged elsewhere in the file stays merged.
#[test]
fn a_git_conflict_on_adjacent_lines_stays_one_and_the_clean_part_comes_along() {
    use crate::persist::merge::{Choice, git_conflict};

    let fixture = fixture("adjacent");
    let dir = fixture.root.join("book");
    let _ = repo_at(&dir);
    let chapter = dir.join("manuscript/one.md");
    let text = |two: &str, three: &str, eight: &str| {
        format!("eins\n{two}\n{three}\nvier\nfünf\nsechs\nsieben\n{eight}\nneun\n")
    };
    let commit = |message: &str| {
        git(&dir, &["add", "."]);
        git(&dir, &["commit", "-q", "-m", message]);
    };
    fs::write(&chapter, text("zwei", "drei", "acht")).unwrap();
    commit("Base");
    git(&dir, &["checkout", "-q", "-b", "probe"]);
    fs::write(&chapter, text("zwei", "DREI", "ACHT")).unwrap();
    commit("Theirs");
    git(&dir, &["checkout", "-q", "main"]);
    fs::write(&chapter, text("ZWEI", "drei", "acht")).unwrap();
    commit("Ours");
    let cli = Git::find().expect("these tests need git");
    let repo = Repo::open(&dir).unwrap().unwrap();
    assert!(matches!(
        cli.merge(&dir, repo.git_dir(), "probe"),
        Ok(Merged::Conflicts)
    ));
    let merged = git_conflict(
        &text("zwei", "drei", "acht"),
        &text("ZWEI", "drei", "acht"),
        &text("zwei", "DREI", "ACHT"),
    );
    assert!(!merged.is_clean());
    assert_eq!(merged.one_side(Choice::Mine), text("ZWEI", "drei", "ACHT"));
}

#[test]
fn only_markdown_counts_words() {
    let fixture = fixture("words");
    let dir = fixture.root.join("book");
    let mut repo = repo_at(&dir);
    fs::write(dir.join("manuscript/one.md"), "# One\n\nFirst words here and more.\n").unwrap();
    fs::write(
        dir.join("literatur.bib"),
        "@misc{a, title={Viele Worte in einer Liste}}\n",
    )
    .unwrap();
    let changes = repo.changes().unwrap();
    let words = |name: &str| (changes.iter().find(|change| change.name == name)).and_then(|change| change.words);
    assert_eq!(words("manuscript/one.md"), Some((2, 0)));
    assert_eq!(words("literatur.bib"), Some((0, 0)));
    repo.commit("Both", None).unwrap();
}
