# Plan — request-logic
Status: Approved 2026-09-30 (auto — verified upstream approval chain)
Tests (in-file): row_table (incl. stock-only), requests (no source row / RANDOM / Within / Key read from the right row / out-of-range / stages), preview_keys (model, source effective, hidden row, RANDOM at either level, unknown), bounds_and_labels.
Implementation: per the design's §4.5 readers, over `Catalog` + `sources` ids.
