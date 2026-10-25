use super::cache_path;
use std::path::Path;

#[test]
fn normal_tofu_root_stays_inside_the_admitted_live_cache_boundary() {
    let runner_temp = Path::new("/home/runner/_work/_temp");
    let path = cache_path(runner_temp, "stacks/provider-a").expect("checked provider path");
    let boundary = runner_temp.join("velnor/cache/tofu");

    assert_eq!(path.parent(), Some(boundary.as_path()));
    assert!(path.file_name().is_some_and(|name| {
        name.to_str().is_some_and(|text| {
            text.starts_with("b3-")
                && text.len() == 67
                && text[3..]
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
    }));
}
