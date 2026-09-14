# Plan — Step 2 Task 02: manifest + plan
Status: Approved 2026-09-13 (auto; approved chain)
Tests — manifest: round trip (write→read equal); absent → Ok(None); garbage → Err; unknown schema → Err; needs_update truth table (8 rows); sha256_file known vector (`abc` → ba7816bf…); hash_tree over 2 files.
Tests — plan: first run (previous None) → Write for every stage file except merged + exe, no Prune, `existed` from probe, `new_manifest_files` == stage hashes minus exe/merged; second run → Prune when hash equal, KeepModified when differs, nothing when Missing; unknown on-disk paths never appear (probe is only consulted for stage + previous paths); merged: `WriteMerged` iff bytes Some, never pruned even if in previous manifest and absent from stage; exe never written/pruned; order of action kinds.
Shape per design §4.9/§4.10; `plan::build(stage_files: &[RelPath], stage_hashes, previous: Option<&Manifest>, probe: &dyn Fn(&RelPath) -> DiskState, merged: &MergedFiles) -> Plan`.
