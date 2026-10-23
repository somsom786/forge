//! A realistic Obsidian vault, built on disk, imported through the same scan the app uses: link forms Obsidian users
//! write, non-ASCII and decomposed (macOS) file names, duplicate names in different folders, attachments, sync-conflict
//! copies, Windows line endings, renames, deletions, and files the user marked read-only.

use super::page::Author;
use super::service::{self, Ctx};
use super::store::{KLink, KPage};
use crate::git;
use crate::store::Store;
use std::path::Path;

struct Env {
    store: Store,
    home: tempfile::TempDir,
    _repo: tempfile::TempDir,
    vault: tempfile::TempDir,
    project: String,
}

fn write(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

const HOME: &str = "# Home\n\
See [[Architecture]], [[Architecture|the architecture]], [[Architecture#Storage]] and [[Architecture#^b1]].\n\
Embedded: ![[Architecture]] and an attachment ![[diagram.png]].\n\
Folders: [[design/Caching]] and with the extension [[Caching.md]].\n\
Unicode: [[日本語]], [[中文]], [[Café]], [[über]].\n\
Duplicates: [[Index]] and [[a/Index]].\n\
Later: [[Renamed Later]]. Missing: [[Nowhere]].\n\
```\n[[Inside a fence]]\n```\n";

impl Env {
    fn new() -> Env {
        let store = Store::in_memory();
        let repo = git::testutil::init_repo();
        let home = tempfile::tempdir().unwrap();
        let p = store.add_project("demo", &repo.path().to_string_lossy(), "main").unwrap();
        let vault = tempfile::tempdir().unwrap();
        let v = vault.path();
        write(v, "Home.md", HOME);
        // Obsidian links by file name; the frontmatter title differs.
        write(v, "Architecture.md", "---\ntitle: System architecture\naliases: [Arch]\n---\n## Storage\nSQLite in WAL mode. ^b1\n");
        write(v, "design/Caching.md", "LRU, 64 entries.\n");
        write(v, "日本語.md", "日本語のメモ\n");
        write(v, "中文.md", "中文笔记\n");
        write(v, "Cafe\u{301}.md", "decomposed file name, as macOS writes it\n");
        write(v, "Über.md", "capitalised umlaut\n");
        write(v, "Caf.md", "a different note whose ASCII slug would collide with Café\n");
        write(v, "a/Index.md", "index a\n");
        write(v, "b/Index.md", "index b\n");
        write(v, "Renamed Later.md", "will be renamed\n");
        write(v, "Windows.md", "\u{feff}---\r\ntitle: Written on Windows\r\n---\r\nCRLF body with [[Home]].\r\n");
        write(v, "Plan (Sam's conflicted copy 2026-10-23).md", "a Dropbox conflict copy\n");
        write(v, "Plan.sync-conflict-20261023-101010-ABCDEFG.md", "a Syncthing conflict copy\n");
        write(v, "Plan.md", "the plan\n");
        write(v, ".trash/Deleted.md", "in Obsidian's trash\n");
        write(v, ".obsidian/workspace.json", "{}");
        std::fs::write(v.join("diagram.png"), [0x89, b'P', b'N', b'G', 0, 0]).unwrap();
        let e = Env { store, home, _repo: repo, vault, project: p.id };
        service::set_vault(e.ctx(), &e.project, Some(e.vault.path()), false).unwrap();
        e
    }
    fn ctx(&self) -> Ctx<'_> {
        Ctx { store: &self.store, home: self.home.path() }
    }
    fn page(&self, rel: &str) -> KPage {
        let want = rel.to_string();
        self.store.k_list(&self.project).unwrap().into_iter().find(|p| p.rel_path == want).unwrap_or_else(|| panic!("no page {rel}"))
    }
    fn links(&self, rel: &str) -> Vec<KLink> {
        self.store.k_links(&self.page(rel).id).unwrap()
    }
}

fn link<'a>(links: &'a [KLink], target: &str) -> &'a KLink {
    links.iter().find(|l| l.target == target).unwrap_or_else(|| panic!("no link to {target}: {links:?}"))
}

#[test]
fn a_realistic_vault_imports_and_its_links_resolve_like_obsidian() {
    let e = Env::new();
    let rep = service::scan(e.ctx(), &e.project).unwrap();
    // 15 notes outside .trash and .obsidian; the PNG is not a note.
    assert_eq!(rep.indexed, 15, "{rep:?}");
    let pages = e.store.k_list(&e.project).unwrap();
    assert!(pages.iter().all(|p| !p.rel_path.starts_with('.')), "trash and config are not imported");
    assert_eq!(e.page("Windows.md").title, "Written on Windows", "BOM and CRLF frontmatter");

    let l = e.links("Home.md");
    let arch = e.page("Architecture.md").id;
    assert_eq!(link(&l, "Architecture").to_page.as_deref(), Some(arch.as_str()), "by file name although the title differs; alias/heading/block/embed forms collapse onto it");
    assert_eq!(link(&l, "design/Caching").to_page, Some(e.page("design/Caching.md").id));
    assert_eq!(link(&l, "Caching.md").to_page, Some(e.page("design/Caching.md").id), "a link written with the .md extension");
    assert_eq!(link(&l, "日本語").to_page, Some(e.page("日本語.md").id), "non-ASCII names resolve, and not to each other");
    assert_eq!(link(&l, "中文").to_page, Some(e.page("中文.md").id));
    assert_eq!(link(&l, "Café").to_page, Some(e.page("Cafe\u{301}.md").id), "a composed link finds a decomposed file name, and not Caf.md");
    assert_eq!(link(&l, "über").to_page, Some(e.page("Über.md").id), "case-insensitive beyond ASCII");
    let idx = link(&l, "Index");
    assert!(idx.ambiguous && idx.to_page.is_none(), "two notes named Index: reported, not guessed");
    assert_eq!(link(&l, "a/Index").to_page, Some(e.page("a/Index.md").id), "a folder path disambiguates");
    let png = link(&l, "diagram.png");
    assert!(png.to_page.is_none() && !png.ambiguous, "an attachment is not a note");
    assert!(link(&l, "Nowhere").to_page.is_none());
    assert!(!l.iter().any(|x| x.target == "Inside a fence"));
    assert_eq!(e.store.k_backlinks(&e.page("Home.md").id).unwrap().len(), 1, "Windows.md links back");

    // Sync-conflict copies are imported (they are the user's text) but called out.
    for name in ["Plan (Sam's conflicted copy 2026-10-23).md", "Plan.sync-conflict-20261023-101010-ABCDEFG.md"] {
        assert!(rep.warnings.iter().any(|w| w.starts_with(name) && w.contains("sync conflict")), "{name}: {:?}", rep.warnings);
    }
    assert!(!rep.warnings.iter().any(|w| w.starts_with("Plan.md")), "the original is not flagged");
}

#[test]
fn renames_and_deletions_in_the_vault_are_followed_on_the_next_scan() {
    let e = Env::new();
    service::scan(e.ctx(), &e.project).unwrap();
    let old = e.page("Renamed Later.md");
    assert!(!e.store.k_search(&e.project, "中文笔记", 10).unwrap().is_empty(), "precondition: the note is searchable");
    let v = e.vault.path();
    std::fs::rename(v.join("Renamed Later.md"), v.join("Renamed.md")).unwrap();
    std::fs::remove_file(v.join("中文.md")).unwrap();
    let rep = service::scan(e.ctx(), &e.project).unwrap();
    assert_eq!(rep.removed, 2, "{rep:?}");
    assert!(e.store.k_get(&old.id).unwrap().missing, "the old path is marked missing, not silently kept");
    assert!(!e.page("Renamed.md").missing);
    let l = e.links("Home.md");
    assert!(link(&l, "Renamed Later").to_page.is_none(), "a link to a renamed note no longer points at a page");
    assert!(link(&l, "中文").to_page.is_none(), "or at a deleted one");
    assert!(e.store.k_search(&e.project, "中文笔记", 10).unwrap().is_empty(), "a deleted note is not searchable");
    // It comes back.
    write(v, "中文.md", "中文笔记\n");
    service::scan(e.ctx(), &e.project).unwrap();
    assert_eq!(link(&e.links("Home.md"), "中文").to_page, Some(e.page("中文.md").id));
}

#[test]
fn a_note_the_user_marked_read_only_is_never_rewritten_or_deleted() {
    let e = Env::new();
    service::set_vault(e.ctx(), &e.project, Some(e.vault.path()), true).unwrap();
    service::scan(e.ctx(), &e.project).unwrap();
    let file = e.vault.path().join("Plan.md");
    let mut perm = std::fs::metadata(&file).unwrap().permissions();
    perm.set_readonly(true);
    std::fs::set_permissions(&file, perm.clone()).unwrap();

    let pg = e.page("Plan.md");
    let rev = service::propose_revision(e.ctx(), &pg.id, "rewritten by FORGE\n", Author::Agent, None).unwrap();
    let err = service::apply_revision(e.ctx(), &rev.id).unwrap_err().to_string();
    assert!(err.contains("read-only"), "{err}");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "the plan\n");
    assert!(service::forget_page(e.ctx(), &pg.id).is_err(), "forgetting would delete a file the user protected");
    assert!(file.exists());
    assert!(!e.store.k_get(&pg.id).unwrap().missing, "a refused forget leaves the page as it was");

    #[allow(clippy::permissions_set_readonly_false)]
    perm.set_readonly(false);
    std::fs::set_permissions(&file, perm).unwrap();
    service::apply_revision(e.ctx(), &rev.id).unwrap();
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "rewritten by FORGE\n");
}
