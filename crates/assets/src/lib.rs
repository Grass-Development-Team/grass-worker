use std::borrow::Cow;
use rust_embed::Embed;

#[derive(Embed)]
#[folder = "assets/public"]
struct EmbeddedPublicAssets;

pub fn get_asset(path: &str) -> Option<Cow<'static, [u8]>> {
    let normalized_path = path.trim_start_matches('/');

    EmbeddedPublicAssets::get(normalized_path).map(|asset| asset.data)
}

#[cfg(test)]
mod tests {
    use super::get_asset;

    #[test]
    fn embedded_public_index_is_available() {
        let asset = get_asset("index.html").expect("embedded public index should exist");
        let index = std::str::from_utf8(asset.as_ref()).expect("index.html should be utf-8");
        let bundle_path =
            referenced_javascript_path(index).expect("index.html should reference a js bundle");
        let leading_slash_bundle_path = format!("/{}", bundle_path.trim_start_matches('/'));
        let normalized_bundle_path = bundle_path.trim_start_matches('/');

        assert!(index.contains("grass-worker"));

        let bundle = get_asset(normalized_bundle_path).expect("referenced js bundle should exist");
        let slash_prefixed_bundle = get_asset(&leading_slash_bundle_path)
            .expect("bundle lookup with leading slash should work");

        assert!(!bundle.is_empty(), "referenced js bundle should not be empty");
        assert_eq!(bundle, slash_prefixed_bundle);
    }

    #[test]
    fn referenced_javascript_path_prefers_script_src() {
        let index = r#"
            <img src="/assets/not-the-app.js" />
            <script src="/assets/not-javascript.css"></script>
            <script type="module" crossorigin src="/assets/index-real.js"></script>
        "#;

        assert_eq!(
            referenced_javascript_path(index),
            Some("/assets/index-real.js")
        );
    }

    fn referenced_javascript_path(index: &str) -> Option<&str> {
        for script_fragment in index.split("<script").skip(1) {
            let tag_end = script_fragment.find('>')?;
            let tag = &script_fragment[..tag_end];
            let src_start = tag.find(r#"src=""#)? + r#"src=""#.len();
            let src_end = tag[src_start..].find('"')? + src_start;
            let path = &tag[src_start..src_end];

            if path.ends_with(".js") {
                return Some(path);
            }
        }

        None
    }
}
