//! SMX HID protocol — pure packet framing and wire encoders (no IO).
//!
//! Byte-for-byte port of the used subset of the StepManiaX SDK's wire layer
//! (`stepmaniax-sdk/sdk/Windows/SMXDeviceConnection.cpp` + `SMXManager.cpp`).
//! Everything here is a pure function over byte buffers so the wire behavior
//! can be read side-by-side against the SDK source (design NFR-6). Validation
//! is on-device (decision D12) — no host tests by design.
//!
//! ## HID report layout (64-byte reports)
//!
//! - report id **3** = input state: `mask = (buf[2] << 8) | buf[1]` (9-bit
//!   panel mask, reading order `012/345/678`).
//! - report id **5** = host→device serial packet: `[5, flags, len, payload…]`,
//!   payload ≤ 61 bytes/packet, commands split across packets with
//!   START/END flags.
//! - report id **6** = device→host serial packet: `[6, flags, len, payload…]`.

/// Full HID report length. Windows pads every report to the largest report
/// size in the device's descriptor, which is 64 for SMX.
pub const HID_REPORT_LEN: usize = 64;

/// Max serial payload bytes per report-id-5 packet (64 - id - flags - len).
pub const MAX_PACKET_PAYLOAD: usize = 61;

pub const INPUT_REPORT_ID: u8 = 3;
pub const HOST_CMD_REPORT_ID: u8 = 5;
pub const SERIAL_REPORT_ID: u8 = 6;

pub const FLAG_END_OF_COMMAND: u8 = 0x01;
pub const FLAG_HOST_CMD_FINISHED: u8 = 0x02;
pub const FLAG_START_OF_COMMAND: u8 = 0x04;
pub const FLAG_DEVICE_INFO: u8 = 0x80;

/// Parse a report-id-3 input report into the 9-bit panel mask.
/// Returns `None` for any other report id or an undersized buffer.
pub fn parse_input_report(report: &[u8]) -> Option<u16> {
    if report.len() < 3 || report[0] != INPUT_REPORT_ID {
        return None;
    }
    Some(((report[2] as u16) << 8) | report[1] as u16)
}

/// A parsed report-id-6 device→host serial packet.
pub struct SerialPacket {
    pub device_info: bool,
    pub start_of_command: bool,
    pub end_of_command: bool,
    pub host_cmd_finished: bool,
    /// The packet's payload bytes (already length-clamped).
    pub payload: Vec<u8>,
}

/// Parse a report-id-6 serial packet. Returns `None` for other report ids,
/// undersized buffers, or an oversized inner length (the SDK logs
/// "oversized packet" and drops it — we mirror that by returning `None`).
pub fn parse_serial_report(report: &[u8]) -> Option<SerialPacket> {
    if report.len() < 3 || report[0] != SERIAL_REPORT_ID {
        return None;
    }
    let flags = report[1];
    let len = report[2] as usize;
    if 3 + len > report.len() {
        return None; // oversized (corrupt) — drop
    }
    Some(SerialPacket {
        device_info: flags & FLAG_DEVICE_INFO != 0,
        start_of_command: flags & FLAG_START_OF_COMMAND != 0,
        end_of_command: flags & FLAG_END_OF_COMMAND != 0,
        host_cmd_finished: flags & FLAG_HOST_CMD_FINISHED != 0,
        payload: report[3..3 + len].to_vec(),
    })
}

/// Split a serial command into report-id-5 HID output reports with
/// START/END flags — the exact chunking of `SMXDeviceConnection::SendCommand`.
/// A zero-length command still produces one (START|END, len 0) packet.
pub fn frame_serial_command(cmd: &[u8]) -> Vec<[u8; HID_REPORT_LEN]> {
    let mut packets = Vec::new();
    let mut i = 0usize;
    loop {
        let chunk = (cmd.len() - i).min(MAX_PACKET_PAYLOAD);
        let mut flags = 0u8;
        if i == 0 {
            flags |= FLAG_START_OF_COMMAND;
        }
        if i + chunk == cmd.len() {
            flags |= FLAG_END_OF_COMMAND;
        }
        let mut packet = [0u8; HID_REPORT_LEN];
        packet[0] = HOST_CMD_REPORT_ID;
        packet[1] = flags;
        packet[2] = chunk as u8;
        packet[3..3 + chunk].copy_from_slice(&cmd[i..i + chunk]);
        packets.push(packet);
        i += chunk;
        if i >= cmd.len() {
            break;
        }
    }
    packets
}

/// The special device-info request packet (`RequestDeviceInfo`): a report-id-5
/// packet with only the DEVICE_INFO flag set and no payload. Safe to send even
/// while another application talks to the device.
pub fn device_info_request() -> [u8; HID_REPORT_LEN] {
    let mut packet = [0u8; HID_REPORT_LEN];
    packet[0] = HOST_CMD_REPORT_ID;
    packet[1] = FLAG_DEVICE_INFO;
    packet
}

/// Parsed device-info response (the DEVICE_INFO-flagged id-6 payload —
/// the SDK's `data_info_packet`).
#[derive(Clone, Copy, Debug)]
pub struct DeviceInfo {
    /// True if this controller reports itself as player 2.
    pub player2: bool,
    /// Master firmware version (drives the lights command shape: ≥ 4 sends
    /// the '4' inner-grid command and queues all three commands at once).
    pub firmware_version: u16,
}

/// Parse a DEVICE_INFO payload: `'I', size, player('0'/'1'), pad, serial[16],
/// fw_version u16 LE, '\n'`. Tolerates the SDK-known one-byte-short packet
/// (23 bytes of data for the 24-byte struct) by zero-padding.
pub fn parse_device_info(payload: &[u8]) -> Option<DeviceInfo> {
    let mut buf = [0u8; 24];
    if payload.is_empty() {
        return None;
    }
    let n = payload.len().min(24);
    buf[..n].copy_from_slice(&payload[..n]);
    Some(DeviceInfo {
        player2: buf[2] == b'1',
        firmware_version: u16::from_le_bytes([buf[20], buf[21]]),
    })
}

// ── Stage lights ─────────────────────────────────────────────────────

/// One pad's full light set: 9 panels × 25 LEDs × RGB, panel reading order
/// `012/345/678`, per panel 16 outer (4×4 rows) then 9 inner (3×3).
pub const PAD_LIGHT_BYTES: usize = 9 * 25 * 3;
pub type PadLights = [u8; PAD_LIGHT_BYTES];

/// Brightness scale applied to every color byte on the wire. Values over
/// ~170 don't make the LEDs brighter; the SDK scales for contrast/power.
#[inline]
fn scale_light(color: u8) -> u8 {
    (color as f32 * 0.6666) as u8
}

/// Encode one pad's lights into the three tagged serial commands:
/// `[0]` = `'4'` + inner 3×3 (fw ≥ 4 only), `[1]` = `'2'` + top 4×2,
/// `[2]` = `'3'` + bottom 4×2 — each panel-major with the ×0.6666 scale and
/// a trailing `'\n'`. Mirrors `SMXManager::SetLights` exactly.
pub fn encode_stage_commands(pad: &PadLights) -> [Vec<u8>; 3] {
    let mut cmd4 = vec![b'4'];
    let mut cmd2 = vec![b'2'];
    let mut cmd3 = vec![b'3'];

    let mut i = 0usize;
    for _panel in 0..9 {
        // 16 outer LEDs: first 4×2 rows (24 bytes) → '2', last 4×2 → '3'.
        for byte in 0..16 * 3 {
            let color = scale_light(pad[i]);
            i += 1;
            if byte < 4 * 2 * 3 {
                cmd2.push(color);
            } else {
                cmd3.push(color);
            }
        }
        // 9 inner LEDs → '4'.
        for _ in 0..9 * 3 {
            cmd4.push(scale_light(pad[i]));
            i += 1;
        }
    }

    cmd4.push(b'\n');
    cmd2.push(b'\n');
    cmd3.push(b'\n');
    [cmd4, cmd2, cmd3]
}

// ── Dedicated cabinet lights ─────────────────────────────────────────

/// The controllable cabinet-light devices of an SMX Dedicated Cabinet
/// (the SDK's `SMXDedicatedCabinetLights` enum; the wire carries the raw
/// index as the command's device byte).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CabinetLightDevice {
    Marquee = 0,
    LeftStrip = 1,
    LeftSpotlights = 2,
    RightStrip = 3,
    RightSpotlights = 4,
}

/// The cabinet lights controller's version/model handshake command. The
/// controller answers with a serial `'I'` response parsed by
/// [`parse_cabinet_info`] (the SDK's `SMXDevice::CheckActive` cabinet path).
pub fn cabinet_info_command() -> &'static [u8] {
    b"I\n"
}

/// Parsed `"I\n"` handshake response.
#[derive(Clone, Copy, Debug)]
pub struct CabinetInfo {
    pub version: u16,
    /// Lights-controller model; selects the wire protocol in
    /// [`encode_cabinet_light`]. Only reported by version ≥ 2 firmware —
    /// older controllers are model 0 (`HandleCabinetInfoResponse`).
    pub model: u8,
}

/// Parse the cabinet controller's `'I'` response: `'I'`, u16 LE version,
/// u8 model (present only when version ≥ 2). Short packets parse as zeroes,
/// mirroring the SDK's `resize(4, 0)`.
pub fn parse_cabinet_info(payload: &[u8]) -> Option<CabinetInfo> {
    if payload.first() != Some(&b'I') {
        return None;
    }
    let mut buf = [0u8; 4];
    let n = payload.len().min(4);
    buf[..n].copy_from_slice(&payload[..n]);
    let version = u16::from_le_bytes([buf[1], buf[2]]);
    Some(CabinetInfo {
        version,
        model: if version >= 2 { buf[3] } else { 0 },
    })
}

/// Wire channel orders (`SMXManager::SetDedicatedCabinetLights`): each
/// output channel is `rgb[order[i]]`.
const ORDER_RGB: [usize; 3] = [0, 1, 2];
const ORDER_BRG: [usize; 3] = [2, 0, 1];
const ORDER_RBG: [usize; 3] = [0, 2, 1];

/// Encode one dedicated-cabinet-lights serial command — a byte-for-byte
/// port of `SMXManager::SetDedicatedCabinetLights`:
///
/// ```text
/// <command char> <light device index> <triplet count> <triplet count × 3 color bytes>
/// ```
///
/// with **no trailing newline** and no brightness scale (unlike the stage
/// commands). `rgb_data` is RGB triplets, full range; the fixed per-device
/// payload is zero-padded past the wire lights. The protocol depends on the
/// controller `model` from the `"I"` handshake:
///
/// - Models 0–2 use `'L'` everywhere (marquee 24 triplets, strips 28).
/// - Model 3 uses `'Q'` for the marquee (20) and strips (23, reversed
///   physical order), `'L'` for the spotlights (6).
/// - Marquee wants B,R,G on the wire; strips want R,B,G (B,R,G on model 1);
///   spotlights are straight RGB.
pub fn encode_cabinet_light(device: CabinetLightDevice, model: u8, rgb_data: &[u8]) -> Vec<u8> {
    let model3 = model == 3;

    let (cmd, mut wire_lights, padded_lights, reverse, order): (u8, usize, usize, bool, _) =
        match device {
            CabinetLightDevice::Marquee => (
                if model3 { b'Q' } else { b'L' },
                if model3 { 20 } else { 24 },
                32,
                false,
                ORDER_BRG,
            ),
            CabinetLightDevice::LeftStrip | CabinetLightDevice::RightStrip => (
                if model3 { b'Q' } else { b'L' },
                if model3 { 23 } else { 28 },
                32,
                model3, // model-3 strips run in the opposite physical direction
                if model == 1 { ORDER_BRG } else { ORDER_RBG },
            ),
            CabinetLightDevice::LeftSpotlights | CabinetLightDevice::RightSpotlights => {
                (b'L', if model3 { 6 } else { 8 }, 8, false, ORDER_RGB)
            }
        };

    // Never read past the data the caller gave us.
    wire_lights = wire_lights.min(rgb_data.len() / 3);

    let mut cmd_buf = Vec::with_capacity(3 + padded_lights * 3);
    cmd_buf.push(cmd);
    cmd_buf.push(device as u8);
    cmd_buf.push(padded_lights as u8);
    for light in 0..wire_lights {
        let source = if reverse {
            wire_lights - 1 - light
        } else {
            light
        };
        let rgb = &rgb_data[source * 3..source * 3 + 3];
        for &channel in &order {
            cmd_buf.push(rgb[channel]);
        }
    }
    // Zero-fill the remainder of the fixed-size payload.
    cmd_buf.resize(3 + padded_lights * 3, 0);
    cmd_buf
}
