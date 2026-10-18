//! Opt-in proof: official local archive candidate equals the unchanged runtime layout.
use super::*;

#[test]
#[ignore = "requires independently verified official local archives and a fresh output directory"]
fn official_archives_candidate_runtime_parity() {
    let input = std::env::var_os("VELNOR_QUALIFICATION_REQUEST").expect("request");
    let output_root = std::env::var_os("VELNOR_QUALIFICATION_PROOF_DIR").expect("fresh proof dir");
    let root = io::resolved_parent(Path::new(&output_root)).expect("absolute proof dir");
    io::directory(&root).expect("fresh proof directory");
    let request_bytes =
        io::read(Path::new(&input), request::MAX_REQUEST_BYTES, deadline()).expect("request");
    let request = request::parse(&request_bytes).expect("strict request");
    let work = root.join("author");
    let output = root.join("candidate.json");
    qualify_tools(Path::new(&input), &work, &output).expect("official authoring");
    let value: Value =
        serde_json::from_slice(&fs::read(&output).expect("candidate")).expect("json");
    let tools: Vec<QualifiedTool> =
        serde_json::from_value(value["qualified_tools"].clone()).expect("tools");
    let runtime = root.join("runtime");
    io::create_home(&runtime).expect("runtime home");
    let mut budget = acquisition::archive::ArchiveBudget::new();
    let until = CheckDeadline::after(Duration::from_secs(3600)).expect("deadline");
    let mut prior = Vec::new();
    for recipe in &request.tools {
        let tool = tools
            .iter()
            .find(|tool| tool.id == recipe.recipe.id)
            .expect("tool");
        let home = runtime.join("tools").join(&tool.id);
        io::directory(&home).expect("home");
        let mut roots = Vec::new();
        for (index, local) in recipe.archives.iter().enumerate() {
            let bytes = io::read(&local.path, 1024 * 1024 * 1024, until).expect("local bytes");
            assert_eq!(
                acquisition::sha256_with_deadline(&bytes, until).expect("archive hash"),
                local.sha256
            );
            assert_eq!(tool.platforms[0].artifacts[index].sha256, local.sha256);
            drop(bytes);
            let unpacked = home.join(format!("unpacked-{index}"));
            acquisition::archive::extract_archive(
                &local.path,
                &unpacked,
                &local.url,
                &mut budget,
                until,
            )
            .expect("runtime extraction");
            roots.push(unpacked);
        }
        let prefix = home.join("prefix");
        acquisition::layout::normalize_payload(tool, native(), &roots, &prefix, until)
            .expect("runtime complete pin layout");
        acquisition::tree::freeze_tree(&prefix, until).expect("freeze");
        assert_eq!(
            acquisition::tree::tree_sha256(&prefix, until).expect("tree"),
            tool.platforms[0].install_tree_sha256
        );
        probe::observe(tool, &prior, &runtime, native(), until).expect("runtime typed probes");
        prior.push(tool.clone());
    }
    acquisition::validate_installations(&prior, native(), &runtime, until)
        .expect("runtime expected identities");
    println!(
        "Official archive runtime parity: {} tools, {} archives; candidate {}",
        tools.len(),
        value["archives"].as_array().expect("archives").len(),
        output.display()
    );
}
