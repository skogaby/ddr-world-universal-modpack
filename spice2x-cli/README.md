# spice2x-cli

`spice2x-cli` is a command-line interface for controlling a running instance of `spice2x` while a Bemani game is being played. This tool is meant for sending inputs to the game, getting outputs from the game, and taking screenshots of the game. This CLI interacts with `spice2x` via the `SpiceAPI`, which is a simple network protocol that `spice2x` exposes for control over TCP.

The main motivation for this project is to allow an AI agent to autonomously test new features while working on development related to Bemani games. This could range from testing new IO emulation implementations, to testing network functionality for new server implementations, or doing deep dives on reverse engineering tasks. Anything that requires direct control and observance of game state and behavior should be achievable through use of this CLI.

## Project Structure

```
src/
├── main.rs              → Entry point: parse CLI, connect, dispatch, format output
├── cli.rs               → clap derive structs: Cli (global opts), Commands, all subcommands
├── output.rs            → Formatter trait, JsonFormatter (pretty JSON), TextFormatter (human-readable)
├── commands/
│   ├── mod.rs           → Command dispatch (routes Commands to handlers)
│   ├── info.rs          → info avs/launcher/memory
│   ├── control.rs       → control raise/exit/restart/shutdown/reboot
│   ├── buttons.rs       → buttons read/write/write-reset
│   ├── analogs.rs       → analogs read/write/write-reset
│   ├── coin.rs          → coin get/set/insert
│   ├── card.rs          → card insert
│   ├── keypads.rs       → keypads get/write/set
│   ├── capture.rs       → capture get-screens/get-jpg
│   └── lights.rs        → lights read
└── protocol/
    ├── mod.rs           → Re-exports
    ├── connection.rs    → TCP connection, RC4 encryption, null-byte framing, session refresh, retry
    ├── rc4.rs           → RC4 stream cipher (KSA + PRGA)
    ├── request.rs       → Request struct + JSON serialization with monotonic IDs
    ├── response.rs      → Response parsing, ID validation, error extraction
    └── error.rs         → ProtocolError enum (thiserror)
```

## Dependencies

| Crate | Purpose |
|-------|---------|
| `clap` (derive) | CLI argument parsing |
| `serde` + `serde_json` | JSON serialization/deserialization |
| `anyhow` | Application-level error handling |
| `thiserror` | Typed protocol errors |
| `base64` | Base64 decode for screenshots |
| `chrono` | Timestamp for auto-generated filenames |

## Building

```bash
cargo build                    # Debug build
cargo build --release          # Release build (optimized)
```

## Testing

```bash
cargo test                     # Run all unit tests
cargo test -- --nocapture      # Run tests with stdout visible
```

No running spice2x instance is needed for unit tests — they test serialization, parsing, RC4, CLI parsing, output formatting, and param construction in isolation.

## Running

Requires a running spice2x instance with SpiceAPI enabled on the target host/port.

### Global Flags

Global flags go before the subcommand:

```
spice2x-cli [--host HOST] [--port PORT] [--password PASS] [--format FORMAT] <command> <subcommand> [args]
```

| Flag | Default | Description |
|------|---------|-------------|
| `--host` | `localhost` | Host address of the spice2x instance |
| `--port` | `1337` | Port of the SpiceAPI server |
| `--password` | *(empty)* | Password for RC4 encryption (empty = plaintext) |
| `--format` | `json` | Output format: `json` or `text` |

### Commands

#### info — System Information

```bash
spice2x-cli info avs           # AVS system info (model, dest, spec, rev, ext)
spice2x-cli info launcher      # Launcher info (version, compile date, system time, args)
spice2x-cli info memory        # Memory usage (physical and virtual)
```

#### control — Process Lifecycle

```bash
spice2x-cli control raise SIGINT     # Raise a signal
spice2x-cli control exit             # Exit with code 0
spice2x-cli control exit 1           # Exit with specific code
spice2x-cli control restart          # Restart the process
spice2x-cli control shutdown         # Shut down the machine
spice2x-cli control reboot           # Reboot the machine
```

Destructive commands (exit, restart, shutdown, reboot) suppress the expected connection closure error.

#### buttons — Digital Button States

```bash
spice2x-cli buttons read                              # Read all button states
spice2x-cli buttons write BT_A=1.0                    # Press a single button
spice2x-cli buttons write BT_A=1.0 BT_B=1.0 BT_START=1.0  # Press multiple buttons at once
spice2x-cli buttons write-reset                        # Reset all buttons
spice2x-cli buttons write-reset BT_A BT_B             # Reset specific buttons
```

The `write` subcommand accepts one or more `NAME=STATE` pairs (state is 0.0 or 1.0). All pairs are sent in a single SpiceAPI request, so multiple buttons are written atomically — ideal for remote control scenarios that require simultaneous inputs.

#### analogs — Analog Inputs

```bash
spice2x-cli analogs read                               # Read all analog values
spice2x-cli analogs write TURNTABLE=0.5                # Set a single analog
spice2x-cli analogs write TURNTABLE=0.5 SLIDER=0.75   # Set multiple analogs at once
spice2x-cli analogs write-reset                        # Reset all analogs
spice2x-cli analogs write-reset SLIDER                 # Reset specific analogs
```

Like buttons, the `write` subcommand accepts multiple `NAME=VALUE` pairs in a single request.

#### coin — Coin Management

```bash
spice2x-cli coin get           # Get current coin count
spice2x-cli coin set 10        # Set coin count to 10
spice2x-cli coin insert        # Insert 1 coin
spice2x-cli coin insert 5      # Insert 5 coins
```

#### card — Virtual Card

```bash
spice2x-cli card insert 0 E004123456789ABC    # Insert card on unit 0 with 16-char hex ID
```

#### keypads — Keypad Input

```bash
spice2x-cli keypads get 0                # Get pressed keys on keypad 0
spice2x-cli keypads write 0 1234         # Type "1234" on keypad 0 (with timing delays)
spice2x-cli keypads set 0 1 3 A          # Set keys 1, 3, A pressed simultaneously
```

Valid key characters: `0`–`9`, `A` (00 key), `D` (decimal key).

#### capture — Screenshots

```bash
spice2x-cli capture get-screens                                    # List available screen indices
spice2x-cli capture get-jpg                                        # Save to capture/capture_YYYYMMDD_HHMMSS.jpg
spice2x-cli capture get-jpg --output-path shot.jpg                 # Save to specific file path
spice2x-cli capture get-jpg --output-folder /tmp/caps              # Save auto-named file to /tmp/caps/
spice2x-cli capture get-jpg --base64                               # Output base64 to stdout
spice2x-cli capture get-jpg --screen 1 --quality 90                # Screen 1, quality 90
spice2x-cli capture get-jpg --divide 2                             # Half-size image
```

By default, screenshots are saved to the `capture/` directory (created automatically). Use `--output-path` for a specific file path, or `--output-folder` to change the directory while keeping auto-generated filenames.

When saving to file, the file path is printed to stderr so stdout stays clean for piping.

#### lights — Light States

```bash
spice2x-cli lights read                    # Read all light states
spice2x-cli lights read TOP_LED SIDE_LED   # Read specific lights
```

### Output Formats

**JSON** (default) — structured data for scripting and piping:
```bash
spice2x-cli coin get                       # Outputs: 5
spice2x-cli buttons read | jq '.[0].name'  # Extract with jq
```

**Text** — human-readable key-value pairs:
```bash
spice2x-cli --format text info avs
# model: LDJ
# dest: J
# spec: A
```

Errors always go to stderr regardless of format. Non-zero exit code on failure.

## Protocol

SpiceAPI uses JSON messages over TCP with null-byte (`\x00`) framing and optional RC4 encryption. On connect, a session refresh exchanges a new cipher password with the server. Failed requests are retried up to 3 times on transient connection errors; API-level errors and ID mismatches are never retried.
