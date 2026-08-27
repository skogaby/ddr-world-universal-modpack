# Progress: string-field-wire

Updated: 2026-08-18
Status: Complete (uncommitted — maintainer commits manually); cabinet round-trip pending (deploy #4 after Step 7)

- custom_options_persistence extension: StringSaveFn/StringLoadFn registry
  (`register_string_field`), `register_card_in_callback`; FnXmlAddChildStr
  second transmute of ordinal 163 (str value = pointer, Ghidra-verified
  convention); emit_string_fields post-original.call under PERSIST_NETWORK;
  load-receiver str reads (ordinal 176, type 11, 64KiB buffer, <0=absent)
  into PENDING_STRING_LOADS (ddrcode-keyed); drain order at SONG_SELECT
  entry: card-in resets (now also fire registered card-in callbacks,
  side-resolved) → s32 loads → string loads.
- Mod wiring (persistence.rs): save = entered side's encode (None un-armed,
  Some("") = server-clear), load = apply_server_string + stats WARN,
  card-in = reset_to_baseline. Registration idempotent, all fns gated on
  is_active.
- Validation: check clean, harness 23/23, release build clean.
- Cabinet: emit observable pre-Step-7 (server ignores unknown child); full
  round-trip validated at deploy #4.

Status: Complete (uncommitted — maintainer commits manually)
