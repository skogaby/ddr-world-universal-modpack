# Requirements: WebUI Customization Options

## Overview

Add in-game custom options for all player customization categories (appeal boards, backgrounds, characters, lanes, lane covers, BGM, and the two unknown categories 6/7) that are normally only configurable through Konami's web portal. Each option presents a scalar row in the custom options UI, backed by runtime discovery of available assets on disk. Selections are applied immediately to the in-memory Customize object and persisted via both the network custom options hook and mod-config.json.

## User Stories

### US-1: In-game customization selection

**As a** DDR World player
**I want** to change my cosmetic customizations (appeal board, background, character, lanes, lane covers, BGM) directly from the in-game options menu
**So that** I don't need access to Konami's web portal to personalize my gameplay experience

**Acceptance Criteria:**

- [ ] A scalar option row exists for each of the following categories: appeal board, background, character, lane (single), lane (double), lane cover (single), lane cover (double), category 6, category 7, BGM
- [ ] Each option row has a generated label texture following the project's existing texture generation convention (see `./scripts/`)
- [ ] Each option row displays the current selection as a numeric value and allows cycling through all available IDs
- [ ] The selectable range for each option is determined at runtime by scanning the game's filesystem for available assets (ARC/IFS files on disk)
- [ ] IDs outside the game's compiled-in max validator range are included (e.g., 9000+ series, 100000+ appeal boards) — the mod writes directly to Customize fields, bypassing setter bounds checks
- [ ] The option values presented are a contiguous sequential range (0, 1, 2, ..., N) that maps internally to the actual non-contiguous asset IDs discovered on disk

### US-2: Immediate visual application

**As a** DDR World player
**I want** my customization changes to take effect immediately when I adjust the option
**So that** I can preview different cosmetics without restarting the game or waiting for a new session

**Acceptance Criteria:**

- [ ] When the player changes a customization option value, the corresponding field in the `ddr::player::Customize` object is written immediately
- [ ] The write targets the Customize object directly (bypassing the vtable setters and their bounds checks)
- [ ] The Customize object offset from PlayerWork is detected at runtime (version-agnostic — works on both 20250805 and 20260324 builds)
- [ ] Changes are visually reflected the next time the game reads the relevant Customize field (next scene/frame that references it)

### US-3: Dual persistence

**As a** DDR World player
**I want** my customization selections to persist across sessions
**So that** I don't have to re-select my preferences every time I play

**Acceptance Criteria:**

- [ ] Selections are persisted to the backend server via the existing custom options network hook
- [ ] Selections are also persisted to mod-config.json as a fallback, with separate entries for P1 and P2
- [ ] On session start, if network values are available they are used; otherwise mod-config.json values are loaded (per-player)
- [ ] Players on servers that don't support the custom options fields still get persistence via mod-config.json alone

### US-4: Version-agnostic runtime discovery

**As a** modpack maintainer
**I want** the available customization IDs to be discovered dynamically from the game's filesystem at runtime
**So that** new assets shipped in future game updates are automatically available without mod code changes

**Acceptance Criteria:**

- [ ] On mod initialization, the mod scans the game's filesystem for custom asset files matching each category's naming convention (e.g., `data/arc/custom/appeal_board/appeal_board_%04d.arc`)
- [ ] The scan produces a sorted list of available asset IDs per category
- [ ] The sequential-to-asset-ID mapping is built from this sorted list
- [ ] If no assets are found for a category, that option row is not registered (graceful degradation)

## Out of Scope

- Custom textures for individual option *values* (scalar numeric display only for now; row labels are generated)
- Web UI implementation on the backend server
- In-options preview thumbnails showing what each asset ID looks like before selecting it (the actual game visuals will update immediately for any customization that affects the current scene)
- Modifying the game's compiled-in max validator values (we bypass setters entirely)
- Sending customize data through the vanilla `playerdata_save` customize pathway (we use the mod's custom options hook instead)
- Identifying what categories 6 and 7 actually control (user will test empirically)

## Open Questions

- None at this time — the RE research document covers the technical unknowns sufficiently for design.

## Dependencies

- Existing `custom_options` service (from feature `20260506-custom-options-support`) for option row registration and network persistence
- Existing `player_work_table` signature for accessing the PlayerWork pointer at runtime
- Game filesystem access for asset discovery (ARC files under `data/arc/custom/`)

## Assumptions

- The Customize object field layout (+0x0C through +0x34) is stable across current and future game versions (only the PlayerWork→Customize offset shifts)
- Writing directly to Customize fields (bypassing vtable setters) does not cause side effects — the game reads these fields on demand without caching
- The asset naming conventions (`appeal_board_%04d.arc`, `lane_single_%04d.arc`, etc.) are stable across game versions
- The existing custom options service supports scalar option rows with arbitrary value ranges
