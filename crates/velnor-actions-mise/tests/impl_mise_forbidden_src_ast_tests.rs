use super::check_filesystem;

fn rejected(name: &str, source: &str) {
    let mut violations = Vec::new();
    check_filesystem(name, source, &mut violations);
    assert!(!violations.is_empty(), "accepted {name}: {source}");
}

#[test]
fn arbitrary_same_file_and_imported_alias_writes_are_rejected() {
    for source in [
        "fn rogue(p: &Path) { fs::write(p, b\"x\")?; }",
        "use std::fs::write as save; fn rogue(p: &Path) { save(p,b\"x\")?; }",
        "fn rogue(p: &Path) { let save = std::fs::write; save(p,b\"x\")?; }",
        "use std::fs::*; fn rogue(p: &Path) { write(p,b\"x\")?; }",
        "use external::unknown_writer as save; fn rogue(p: &Path) { save(p,b\"x\")?; }",
        "fn rogue(p: &Path) { std::fs::File::create(p)?; }",
        r#"fn rogue(p:&Path) { let f=std::fs::OpenOptions::new().append(true).open(p)?; writeln!(f,"x")?; }"#,
        r#"fn rogue(p:&Path) { let f=std::fs::OpenOptions::new().read(true).custom_flags(0x200).open(p)?; }"#,
        "fn rogue(root:&mut PrivateGitRoot, p:PathBuf) { root.path=p; }",
        "fn rogue(root:&mut PrivateGitRoot, p:PathBuf) { let alias=root; alias.files[0]=p; }",
        "fn rogue(root:&mut PrivateGitRoot, p:PathBuf) { std::mem::replace(&mut root.path,p); }",
        r#"fn rogue(p:&Path) { let mut f=std::fs::OpenOptions::new().append(true).open(p)?; std::io::Write::write_all(&mut f,b"x")?; }"#,
    ] {
        rejected("command_git_private_root_files.rs", source);
    }
}

const VALID_WRITE: &str = r#"impl PrivateGitRoot {
fn write_file(&self, slot: OwnedFile, bytes: &[u8]) -> io::Result<()> {
self.verify_binding()?;
let flags = 0x20000 | 0x800;
let path = self.slot_path(slot);
let mut handle = fs::OpenOptions::new().write(true).create_new(true)
.custom_flags(flags).mode(0o600).open(&path)?;
handle.write_all(bytes)?;
Ok(())
}}"#;

#[test]
fn closed_receiver_destinations_admit_only_owned_handles() {
    let mut violations = Vec::new();
    check_filesystem(
        "command_git_private_root_files.rs",
        VALID_WRITE,
        &mut violations,
    );
    assert!(violations.is_empty(), "{violations:?}");
    for (old, new) in [
        ("&self, slot: OwnedFile", "&self, slot: PathBuf"),
        ("self.slot_path(slot)", "other.slot_path(slot)"),
        ("self.slot_path(slot)", "self.path.join(name)"),
        ("self.slot_path(slot)", r#"PathBuf::from("outside")"#),
        (".open(&path)", ".open(caller_path)"),
        (".create_new(true)", ".create(true)"),
        ("handle.write_all(bytes)", "other.write_all(bytes)"),
        (
            "handle.write_all(bytes)",
            "let alias = handle; alias.write_all(bytes)",
        ),
        ("self.verify_binding()?;", "self.verify_binding();"),
        ("let flags = 0x20000 | 0x800;", "let flags = caller_flags;"),
        (
            "let path = self.slot_path(slot);",
            "let path = self.slot_path(slot); let path = caller_path;",
        ),
    ] {
        rejected(
            "command_git_private_root_files.rs",
            &VALID_WRITE.replace(old, new),
        );
    }
    rejected("other.rs", VALID_WRITE);
}

#[test]
fn forged_roots_and_open_slots_are_rejected() {
    for source in [
        "fn rogue() { let root = PrivateGitRoot { path: caller, handle: foreign }; }",
        "pub struct PrivateGitRoot { pub path: PathBuf }",
        "enum OwnedFile { Index, Head, CallerPath(PathBuf) }",
        "impl PrivateGitRoot { fn slot_path(&self, slot: OwnedFile) -> &Path { &self.path } }",
        "impl PrivateGitRoot { fn cleanup(&mut self) { self.verify_binding()?; fs::remove_dir_all(other.path)?; } }",
    ] {
        rejected("command_git_private_root_files.rs", source);
    }
}

#[test]
fn filesystem_scan_requires_valid_ast_and_preserves_read_only_access() {
    rejected("read.rs", "fn broken(");
    let mut violations = Vec::new();
    check_filesystem(
        "read.rs",
        "fn read(p: &Path) { let bytes = std::fs::read(p)?; }",
        &mut violations,
    );
    assert!(violations.is_empty());
}

#[test]
fn every_pattern_binding_replaces_destination_provenance() {
    let base = r#"impl PrivateGitRoot {
fn write_file(&self, slot: OwnedFile, bytes: &[u8]) -> io::Result<()> {
self.verify_binding()?; let flags = 0x20000 | 0x800; let path = self.slot_path(slot);
let options = fs::OpenOptions::new().write(true).create_new(true).custom_flags(flags).mode(0o600);
BODY
Ok(())
}}"#;
    for body in [
        r#"match Path::new("/tmp/unowned") { path => { let mut handle=options.open(path)?; handle.write_all(bytes)?; } }"#,
        r#"for path in [Path::new("/tmp/unowned")] { let mut handle=options.open(path)?; handle.write_all(bytes)?; }"#,
        r#"if let Some(path)=Some(Path::new("/tmp/unowned")) { let mut handle=options.open(path)?; handle.write_all(bytes)?; }"#,
        r#"let (path,)=(Path::new("/tmp/unowned"),); let mut handle=options.open(path)?; handle.write_all(bytes)?;"#,
    ] {
        rejected(
            "command_git_private_root_files.rs",
            &base.replace("BODY", body),
        );
    }
}

#[test]
fn unexpanded_source_and_spoofed_standard_names_are_rejected() {
    for source in [
        r#"include!("writer.inc");"#,
        r#"#[path="../writer.rs"] mod outside;"#,
        r#"#[unknown_writer] fn innocent() {}"#,
        r#"use external::assert; assert!("innocent");"#,
        r#"type Formatter=std::fs::File; fn rogue(f:&mut Formatter) { writeln!(f,"x"); }"#,
        r#"fn rogue<Formatter>(f:&mut Formatter) { writeln!(f,"x"); }"#,
        r#"fn rogue(p:&Path) { let NOFOLLOW_FLAG=0x200; let NONBLOCK_FLAG=0; fs::OpenOptions::new().read(true).custom_flags(NOFOLLOW_FLAG | NONBLOCK_FLAG).open(p)?; }"#,
    ] {
        rejected("other.rs", source);
    }
}

const VALID_REPLACE: &str = r#"impl PrivateGitRoot {
fn replace_contents(&self, slot: OwnedFile, bytes: &[u8]) -> io::Result<()> {
self.verify_binding()?;
let flags = 0x20000 | 0x800;
let path = self.slot_path(slot);
let before = fs::symlink_metadata(path)?;
let mut handle = fs::OpenOptions::new().write(true).custom_flags(flags).open(path)?;
let held = handle.metadata()?;
if !held.file_type().is_file() || held.nlink() != 1 || held.mode() & 0o7777 != 0o600
|| (held.dev(), held.ino()) != (before.dev(), before.ino()) {
return Err(io::Error::other("binding_changed"));
}
let named = fs::symlink_metadata(path)?;
if (named.dev(), named.ino()) != (held.dev(), held.ino()) {
return Err(io::Error::other("binding_changed"));
}
handle.set_len(0)?;
handle.write_all(bytes)?;
Ok(())
}}"#;

#[test]
fn replacement_requires_original_held_descriptor_checks_before_effects() {
    let mut violations = Vec::new();
    check_filesystem(
        "command_git_private_root_files.rs",
        VALID_REPLACE,
        &mut violations,
    );
    assert!(violations.is_empty(), "{violations:?}");
    for (old, new) in [
        ("held.nlink() != 1", "held.nlink() != 0"),
        (
            "held.mode() & 0o7777 != 0o600",
            "held.mode() & 0o7777 != 0o644",
        ),
        (
            "(named.dev(), named.ino()) != (held.dev(), held.ino())",
            "false",
        ),
        (
            "return Err(io::Error::other",
            "let _ = Err(io::Error::other",
        ),
        (
            ".custom_flags(flags).open(path)",
            ".create_new(true).custom_flags(flags).mode(0o600).open(path)",
        ),
        ("handle.set_len(0)", "handle.set_len(1)"),
        ("handle.set_len(0)", "other.set_len(0)"),
        (
            "handle.set_len(0)?;",
            "let handle = other; handle.set_len(0)?;",
        ),
        (
            "let named = fs::symlink_metadata(path)?;",
            "let named = before;",
        ),
    ] {
        rejected(
            "command_git_private_root_files.rs",
            &VALID_REPLACE.replace(old, new),
        );
    }
}

#[test]
fn root_and_filesystem_trait_names_cannot_be_spoofed() {
    for source in [
        "type PrivateGitRoot=FakeRoot;",
        "type OwnedFile=FakeSlot;",
        "enum PrivateGitRoot { Fake(FakeRoot) }",
        "use foreign::MetadataExt;",
        "use foreign::OpenOptionsExt;",
        "trait MetadataExt {}",
        "trait PrivateGitRoot {}",
        "extern crate external as std;",
        "mod r#std {}",
        "use external::stringify;",
        "use external::std;",
        "use external::fs;",
        "use external::serde_json;",
        "union PrivateGitRoot { path: FakeRoot }",
    ] {
        rejected("command_git_private_root_files.rs", source);
    }
}

#[test]
fn replacement_rejects_stale_named_capture_and_alternative_extensions() {
    let stale = VALID_REPLACE
        .replace(
            "let before = fs::symlink_metadata(path)?;",
            "let before = fs::symlink_metadata(path)?; let named = fs::symlink_metadata(path)?;",
        )
        .replace("let named = fs::symlink_metadata(path)?;\nif", "if");
    rejected("command_git_private_root_files.rs", &stale);
    for forged in [
        "use external::FakeMetadata;",
        "use external::FakeOptions;",
        "use super::*;",
        "trait FakeMetadata { fn nlink(&self)->u64; } impl FakeMetadata for std::fs::Metadata { fn nlink(&self)->u64 { 1 } }",
        "trait FakeOptions { fn custom_flags(&mut self, flags:i32)->&mut Self; } impl FakeOptions for std::fs::OpenOptions { fn custom_flags(&mut self, flags:i32)->&mut Self { self } }",
        "impl std::fs::Metadata { fn mode(&self)->u32 { 0o600 } }",
        "trait OtherOptions { fn custom_flags(&mut self, flags:i32)->&mut Self { self } } impl OtherOptions for std::fs::OpenOptions {}",
    ] {
        rejected(
            "command_git_private_root_files.rs",
            &format!("{forged} {VALID_REPLACE}"),
        );
    }
}

#[test]
fn root_projection_methods_cannot_create_hidden_mutable_aliases() {
    for source in [
        "fn rogue(root:&mut PrivateGitRoot,p:PathBuf){for path in root.files.iter_mut(){*path=p.clone();}}",
        "fn rogue(root:&mut PrivateGitRoot,p:PathBuf){root.files.as_mut_slice().fill(p);}",
        "fn rogue(root:&mut PrivateGitRoot,p:PathBuf){let files=&mut root.files;for slot in files.iter_mut(){*slot=p.clone();}}",
        "impl PrivateGitRoot{fn rogue(&mut self,p:PathBuf){let files=&mut self.files;for slot in files.iter_mut(){*slot=p.clone();}}}",
        "fn rogue(root:&mut PrivateGitRoot,p:PathBuf){let path=&mut root.path;*path=p;}",
        "fn rogue(root:&mut PrivateGitRoot,p:PathBuf){let files=&mut root.files;files[0]=p;}",
        "fn rogue(root:&mut PrivateGitRoot,p:PathBuf){root.path.as_mut_os_string().push(p);}",
        "fn rogue(root:&mut PrivateGitRoot,p:PathBuf){root.directories.each_mut()[0].push(p);}",
        "fn rogue(root:&mut PrivateGitRoot){root.identity.0=7;}",
        "fn rogue(root:&mut PrivateGitRoot,p:PathBuf){let PrivateGitRoot{files,..}=root;for path in files.iter_mut(){*path=p.clone();}}",
        "fn rogue(root:&mut PrivateGitRoot,p:PathBuf){let ref mut files=root.files;for path in files.iter_mut(){*path=p.clone();}}",
        "impl PrivateGitRoot{fn rogue(&mut self){let Self{files,..}=self;files.fill(p);}}",
        "trait OtherDisplay { fn display(&mut self) {self.push(\"/unowned\");} } impl OtherDisplay for std::path::PathBuf {}",
    ] {
        rejected("command_git_private_root_files.rs", source);
    }
    let mut found = Vec::new();
    check_filesystem(
        "command_git_private_root_files.rs",
        "fn readonly(root:&PrivateGitRoot){root.path.display();root.handle.metadata();}",
        &mut found,
    );
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn fixed_slot_constants_require_exact_private_declarations_and_bound_symbols() {
    use super::shapes::{Origin, origin, slot_symbols};
    let source = r#"
const DIRECTORY_NAMES: [&str; 5] = ["gitdir", "gitdir/info", "gitdir/refs",
    "gitdir/refs/heads", "gitdir/objects"];
const FILE_NAMES: [&str; 7] = ["index", "gitdir/HEAD", "gitdir/config",
    "gitdir/effective.config", "gitdir/config.fragment", "gitdir/info/attributes",
    "gitdir/info/exclude"];
"#;
    let file = syn::parse_file(source).expect("constant fixture AST");
    let mut symbols = slot_symbols(&file).expect("exact closed constants");
    symbols.insert("path".to_owned(), Origin::AllocRoot);
    for (code, expected) in [
        (
            "DIRECTORY_NAMES.map(|name| path.join(name))",
            Origin::Directories,
        ),
        ("FILE_NAMES.map(|name| path.join(name))", Origin::Files),
        ("FILE_NAMES.map(|name| other.join(name))", Origin::Unknown),
        ("FILE_NAMES.map(|name| path.join(caller))", Origin::Unknown),
        ("[\"index\"].map(|name| path.join(name))", Origin::Unknown),
    ] {
        let expr = syn::parse_str(code).expect("mapper AST");
        assert_eq!(origin(&expr, &symbols, "create", true), expected);
    }
    symbols.insert("FILE_NAMES".to_owned(), Origin::Input);
    let expr = syn::parse_str("FILE_NAMES.map(|name| path.join(name))").expect("shadow AST");
    assert_eq!(origin(&expr, &symbols, "create", true), Origin::Unknown);
    for (old, new) in [
        ("const FILE_NAMES", "pub const FILE_NAMES"),
        ("const FILE_NAMES", "#[cfg(test)] const FILE_NAMES"),
        ("[&str; 7]", "[&str; 8]"),
        ("[&str; 7]", "[&mut str; 7]"),
        ("gitdir/config.fragment", "outside"),
        ("\"index\", \"gitdir/HEAD\"", "\"gitdir/HEAD\", \"index\""),
    ] {
        let bad = syn::parse_file(&source.replace(old, new)).expect("negative constant AST");
        assert!(slot_symbols(&bad).is_none(), "accepted {old} -> {new}");
    }
}
