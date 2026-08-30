//! afplist `<geo>` id-list extension — the pure XML transform.
//!
//! The AFP runtime loads a template's geos strictly from the afplist
//! `<geo>` index list at IFS mount time (cabinet-observed on the
//! s-marvelous deploys: `afp-mip: can not find geo id` for any geo not in
//! the list — there is NO on-demand fallback). Injecting a net-new shape
//! into an existing template therefore requires editing the EXISTING
//! `<afp name=...>` entry's list; the append-only `xml_merger` cannot do
//! that (a duplicate `<afp>` node risks double-registering the whole
//! template with the AFP runtime).
//!
//! This module is `std`-only so the host-test harness
//! (`scripts/validate_s_marvelous.sh`) can mount it. The impure half
//! (registry, cache write, serve path) lives in `ifs_textures`.

/// Extend the `<geo>` id list of the `<afp name="{afp_name}">` entry in an
/// afplist XML (kbin already decoded to text). Appends `extra_ids`
/// (skipping ids already listed) and rewrites the `__count` attribute.
/// Returns `None` when the entry or its `<geo>` node can't be located
/// (callers fall back to the unmodified XML — fail-open).
pub fn extend_afplist_geo(xml: &str, afp_name: &str, extra_ids: &[u16]) -> Option<String> {
    // Locate the <afp ... name="{afp_name}" ...> block.
    let needle = format!("name=\"{}\"", afp_name);
    let mut search = 0usize;
    let afp_start = loop {
        let tag_rel = xml[search..].find("<afp")?;
        let tag_start = search + tag_rel;
        let tag_end = tag_start + xml[tag_start..].find('>')?;
        if xml[tag_start..tag_end].contains(&needle) {
            break tag_start;
        }
        search = tag_end + 1;
    };
    let afp_end = afp_start + xml[afp_start..].find("</afp>")?;

    // Locate the <geo ...>ids</geo> node inside the block.
    let block = &xml[afp_start..afp_end];
    let geo_tag_rel = block.find("<geo")?;
    let geo_tag_end_rel = geo_tag_rel + block[geo_tag_rel..].find('>')?;
    let geo_text_end_rel = block.find("</geo>")?;
    if geo_text_end_rel < geo_tag_end_rel {
        return None; // malformed: </geo> before the open tag ends
    }
    let ids_text = &block[geo_tag_end_rel + 1..geo_text_end_rel];

    let mut ids: Vec<String> = ids_text.split_whitespace().map(str::to_string).collect();
    let before = ids.len();
    for id in extra_ids {
        let s = id.to_string();
        if !ids.contains(&s) {
            ids.push(s);
        }
    }
    if ids.len() == before {
        return Some(xml.to_string()); // nothing to add — already present
    }

    let new_node = format!(
        "<geo __type=\"u16\" __count=\"{}\">{}</geo>",
        ids.len(),
        ids.join(" ")
    );
    Some(format!(
        "{}{}{}",
        &xml[..afp_start + geo_tag_rel],
        new_node,
        &xml[afp_start + geo_text_end_rel + "</geo>".len()..]
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<afplist>
  <afp name="dance_judge_for_freeze">
    <geo __type="u16" __count="2">1 2</geo>
  </afp>
  <afp name="dance_judge">
    <geo __type="u16" __count="3">5 8 41</geo>
  </afp>
</afplist>"#;

    #[test]
    fn extends_the_named_entry_only() {
        let out = extend_afplist_geo(SAMPLE, "dance_judge", &[63]).unwrap();
        assert!(out.contains(r#"<geo __type="u16" __count="4">5 8 41 63</geo>"#));
        // The other entry is untouched.
        assert!(out.contains(r#"<geo __type="u16" __count="2">1 2</geo>"#));
        // Exactly one node changed.
        assert_eq!(out.matches("63").count(), 1);
    }

    #[test]
    fn skips_ids_already_listed() {
        let out = extend_afplist_geo(SAMPLE, "dance_judge", &[41]).unwrap();
        assert_eq!(out, SAMPLE); // byte-identical — nothing to add
        let out = extend_afplist_geo(SAMPLE, "dance_judge", &[41, 63]).unwrap();
        assert!(out.contains(r#"__count="4">5 8 41 63</geo>"#));
    }

    #[test]
    fn multiple_ids_appended_in_order() {
        let out = extend_afplist_geo(SAMPLE, "dance_judge", &[63, 70]).unwrap();
        assert!(out.contains(r#"__count="5">5 8 41 63 70</geo>"#));
    }

    #[test]
    fn name_match_is_exact_not_substring() {
        // "dance_judge" must not match inside "dance_judge_for_freeze"'s tag
        // (needle includes the closing quote).
        let out = extend_afplist_geo(SAMPLE, "dance_judge", &[63]).unwrap();
        assert!(out.contains(
            r#"<afp name="dance_judge_for_freeze">
    <geo __type="u16" __count="2">1 2</geo>"#
        ));
    }

    #[test]
    fn missing_entry_or_geo_fails_open() {
        assert!(extend_afplist_geo(SAMPLE, "nope", &[63]).is_none());
        let no_geo = r#"<afplist><afp name="x"></afp></afplist>"#;
        assert!(extend_afplist_geo(no_geo, "x", &[63]).is_none());
        assert!(extend_afplist_geo("", "x", &[63]).is_none());
    }

    #[test]
    fn count_attribute_absent_still_works() {
        // kbin text may omit __count for single-value nodes.
        let xml = r#"<afplist><afp name="a"><geo __type="u16">7</geo></afp></afplist>"#;
        let out = extend_afplist_geo(xml, "a", &[9]).unwrap();
        assert!(out.contains(r#"<geo __type="u16" __count="2">7 9</geo>"#));
    }
}
