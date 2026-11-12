# Plan — planner-carries-source
Status: Approved 2026-09-30 (auto — verified upstream approval chain)
Tests: existing planner tests migrated to `labels()` (expectations unchanged); new: source_folder_entries_carry_the_source, legacy_placements_land_in_custom, a_custom_folder_merges_with_the_implicit_source, two_spellings_of_one_slug_merge_with_one_note, a_refused_source_skips_its_content_with_one_warning (dancer + stage), duplicate_key_across_sources_keeps_the_first_source.
Implementation: `SourceResolver` (seen slugs / refused folders) called once per PackDir at the top of both loops; `pending_key_labels` carries the SourceRef; `Plan.entries` + `labels()` + `source_counts()`; module doc rewritten to the design's §5.1.
