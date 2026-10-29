# KingSmith WalkingPad Z1 Bluetooth reference

This document describes Bluetooth messages relevant to WalkingPad Z1 interoperability. It covers known messages, not every firmware operation. DeskSteps is unaffiliated with KingSmith.

Validation labels: **device** = observed in attended tests on the unit below; **reference** = a layout or interpretation that is not fully device-validated; **upstream** = open-source reference without established applicability to this unit; **unknown** = unresolved. Synthetic encoding tests do not prove physical behavior.

## Compatibility and GATT

Device-verified identity: advertised name `KS-HD-Z1D`, Device Information model `KS-Z1D`, firmware `J41_V301.08.14`, software `V0.0.6`. Advertised speed range read from `2ad4`: `64 00 58 02 0a 00` = 1.0 minimum, 6.0 maximum, 0.1 increment, all three unsigned LE16 values in 0.01 km/h. Compatibility of other Z1 variants/firmware is unknown.

Short UUIDs below expand to `0000XXXX-0000-1000-8000-00805f9b34fb`.

| Service/characteristic | Direction/use | Evidence |
| --- | --- | --- |
| `1826` | Fitness Machine Service (FTMS) | device |
| `2acd` | Treadmill Data notifications | device |
| `2ada` | Fitness Machine Status notifications | device |
| `2ad3` | Training Status notifications | device |
| `2ad9` | Control Point: write with response, subscribe to indications | device |
| `2acc` | Fitness Machine Feature read | discovered capability channel; do not infer optional support from its existence |
| `2ad4` | Supported Speed Range read | device |
| `180a` | Device Information service | device |
| `2a24`, `2a26`, `2a28` | Model, firmware, software revision reads | device |
| `24e2521c-f63b-48ed-85be-c5330a00fdf7` | KingSmith supplement service | device |
| `24e2521c-f63b-48ed-85be-c5330b00fdf7` | Supplement notifications | device |
| `24e2521c-f63b-48ed-85be-c5330d00fdf7` | Supplement write without response | device |
| `fff0`, `fff1`, `fff2` | Legacy vendor service, notifications and writes respectively | discovered/reference layout; legacy responses not established on this Z1 |
| `ffc0`, `ff00` | Additional discovered vendor services | payload semantics unknown |
| `fe00` | Upstream observation-service lead | do not claim every device exposes it |
| `2902` descriptor | Notification/indication enablement | Windows subscription setup writes descriptors |

Do not use a private Bluetooth address as a model identifier. DeskSteps pins the selected address, then verifies the model name before its supplement handshake. On Windows, empty names/address placeholders can require a bounded active scan; a placeholder itself never authorizes the handshake. The address-placeholder recovery path and repeated power/radio recovery were tested in finite cases; the original earlier recovery failure's internal cause was not isolated.

## Sufficient telemetry initialization

1. Discover the selected pad and inspect its GATT services/characteristics with uncached Windows access.
2. Subscribe to treadmill, relevant status and supplement notifications. Pace subscription setup: discovery used roughly 300 ms between subscriptions following an upstream firmware observation. This is not a universally required firmware timing constant.
3. Write the name-dependent supplement handshake once per connection without response.
4. Require a successful supplement connection reply before crediting samples. A Windows Connected status alone does not establish treadmill capture.
5. Persist fresh received data promptly. While stopped, a handshake can succeed without any treadmill snapshot. Silence is unknown data, never zero steps.

The tested device produced walking packets at roughly one-second intervals after this sequence. Standard Request Control, property queries, legacy statistics polling, user-ID initialization and movement commands were not required for telemetry. A connection-only trial illuminated a sleeping display without explicit reads/subscriptions/commands; Windows can perform implicit service discovery, so the exact wake exchange remains unknown.

### Supplement handshake

Device-verified outgoing bytes:

```text
71 00 05 N LE32(V + N) C
```

`N` is an integer 0–254. `V` is the unsigned little-endian integer encoded by the last four ASCII device-name characters; for `KS-HD-Z1D`, suffix `-Z1D` is `2d 5a 31 44`, hence `V = 0x44315a2d`. Addition wraps at 32 bits. `C` is the sum of the preceding eight bytes modulo 256.

| Nonce | Complete outgoing frame |
| --- | --- |
| 0 | `71 00 05 00 2d 5a 31 44 72` |
| 1 | `71 00 05 01 2e 5a 31 44 74` |
| 254 | `71 00 05 fe 2b 5b 31 44 6f` |

Recognized reply begins `71 80 STATUS`. Zero means the app's connection-success condition; nonzero is rejection. Observed success: `71 80 00 f1`. Its last byte matches the full-frame byte sum, but the implemented recognizer checks prefix/status rather than independently establishing a general receive checksum. Retain raw bytes.

## Treadmill Data (`2acd`)

Read an unsigned little-endian 16-bit flags word, then consume fields **in the following order**, only when present. Most flags mean present when set; bit 0 instead omits instantaneous speed when set. Do not use fixed offsets after optional fields. Reject truncation rather than padding with zeros. Preserve remaining unknown bytes.

| Bit / mask | Bytes | Interpretation |
| --- | --- | --- |
| 0 / `0001` clear | 2 | Instantaneous speed, unsigned LE16 / 100 km/h |
| 1 / `0002` | 2 | Average speed, unsigned LE16 / 100 km/h |
| 2 / `0004` | 3 | Distance, unsigned LE24 metres |
| 3 / `0008` | 4 | Inclination and ramp angle: two signed LE16 / 10, percent and degrees |
| 4 / `0010` | 4 | Elevation-gain group; retained/skipped for alignment, not exposed as verified activity |
| 5 / `0020` | 1 | Instantaneous-pace byte; not interpreted by DeskSteps |
| 6 / `0040` | 1 | Average-pace byte; not interpreted by DeskSteps |
| 7 / `0080` | 5 | Total kcal LE16, kcal/hour LE16, kcal/minute byte (last byte not exposed) |
| 8 / `0100` | 1 | Heart-rate byte, beats/minute |
| 9 / `0200` | 1 | Metabolic-equivalent byte; not interpreted by DeskSteps |
| 10 / `0400` | 2 | Elapsed seconds, unsigned LE16 |
| 11 / `0800` | 2 | Remaining seconds, unsigned LE16 |
| 12 / `1000` | 4 | Belt-force/power group; retained/skipped for alignment |
| 13 / `2000` | 3 | Vendor steps extension, unsigned **LE24**, units of one step |

DeskSteps consumes all three bytes of the bit-13 counter. Some upstream decoders use LE16 and ignore the third byte. Tests above 65,535 cover the encoding difference, not actual firmware overflow; rollover behavior remains unknown. Absence of the field is missing steps, not zero.

Public upstream fixture:

```text
84 24 f4 01 d2 05 00 68 00 00 00 00 3a 05 06 09 00
```

Expected: speed 5.0 km/h, distance 1490 m, device calories 104 kcal, elapsed 1338 s, steps 2310. 

## FTMS controls (`2ad9`)

Subscribe to indications before writing with response. Keep one outstanding request at a time. Obtain Request Control before each explicit action. These frames have no supplement envelope/checksum.

| Operation | Write hex | Successful indication | Tested behavior |
| --- | --- | --- | --- |
| Request Control | `00` | `80 00 01` | Access acceptance, no movement |
| Start / Resume | `07` | `80 07 01` | Starts or resumes; device chooses initial speed |
| Pause | `08 02` | `80 08 01` with optional `02` echo | Stops while retaining nonzero live workout counters |
| Stop | `08 01` | `80 08 01` with optional `01` echo | Clears live steps/time/distance/calories; documented, not exposed by DeskSteps |
| Set speed | `02 LL HH` | `80 02 01` with optional `LL HH` echo | Target is unsigned LE16 in 0.01 km/h |

Examples: 1.0 = `02 64 00`, 1.1 = `02 6e 00`, 2.1 = `02 d2 00`, 3.0 = `02 2c 01`, 6.0 = `02 58 02`. The pad advertises 1.0–6.0 in 0.1 increments; physical PC verification covers 1.0, 1.1 and 3.0 only. Zero is not a substitute for Pause.

Response prefix is `80`, followed by request opcode and result: `01` success, `02` unsupported opcode, `03` invalid parameter, `04` operation failed, `05` control not permitted. Preserve unknown results/extra bytes and correlate to the outstanding command. Observed replies include `80 02 01 6e 00`, `80 08 01 02`, `80 08 01 01`. Stop and Pause share opcode 08, so retain requested operation even when echo is absent.

A successful write is not acceptance; acceptance is not measured motion. Indications and fresh current-connection speed telemetry are separate evidence. On rejection, timeout or uncertain outcome, do not automatically retry movement. Device-side speed can take time to settle. Changing speed during startup has an upstream BLE-disruption report; it was not reproduced on this unit.

The app's responsive speed path uses fresh positive moving telemetry, accumulates
the selected target, and reuses FTMS control access on the current connection.
Speed writes remain serialized at GATT completion but do not wait for speed
indications or settling before sending the next explicit click. Indications remain
raw diagnostic evidence; explicit rejections still mark uncertainty. The latest
target is checked separately against measured telemetry. Start/Resume and Pause
still wait for their matching indications and motion. This rapid-write behavior
is an application policy verified offline, not a device-validated remote protocol
or a claim that the firmware accepts every rapid command.

Machine Status observations: `04` started/resumed, `02 02` paused, `02 01` stopped. Upstream target-speed status `05 LL HH` was not emitted for tested speed changes; use Treadmill Data rather than requiring that status. Training Status supplied a 3/2/1/0 countdown before one Start; do not treat countdown/status as physical movement confirmation.

Resume retained nonzero counters but selected 1.0 after a 3.0 workout. Counter retention does not imply speed retention. Stop produced no retained nonzero treadmill total after the command in the tested observation window: persist the pre-Stop progress rather than expecting a final old-counter packet. Archived-history effects are unknown.

## Supplement read queries and events

Use the supplement write characteristic without response, after successful handshake. Capability reads can affect standby/display behavior; no blanket no-side-effect claim is established.

| Operation | Write hex | Receive class | Evidence |
| --- | --- | --- | --- |
| Read all properties | `72 00 00 00 72` | `72 80 LENGTH ... CHECKSUM` | device reply; no step snapshot |
| Read supported action lists | `75 00 00 75` | `75 80 LENGTH ... CHECKSUM` | device reply |
| Property event | unsolicited | `72 50 LENGTH ... CHECKSUM` | awake/standby transitions observed |
| Settlement event | unsolicited | `73 50 09 ... CHECKSUM` | observed after stop; field meanings unverified |

Do not pad the four-byte action-list query by analogy with the property query. Outgoing supplement wrapper checksum is byte sum modulo 256. Incoming property/action-list read replies matched full-frame checksums, but observed property events matched **payload-only** sums. A universal incoming checksum rule is unresolved.

Property read reply payload: four-byte entries `[property_id, result/error, value_lo, value_hi]`. Successful replies included IDs 1, 2, 4, 5, 6, 8 and 10. This reference uses the provisional labels unit, autoStop, motorVersion, errCode, childLock, otherSwitch and modeStatus respectively; these labels and their complete semantics are not device-validated. Preserve unknown IDs/status/raw values. The autoStop field is a bitfield; it is not established as the idle standby timeout.

The reference interpretation of modeStatus uses bits 8–11 for auto/sleep/course/program support and low-byte bits 5–7 for a mode index: 0 manual, 1 auto, 2 standby, 3 course, 4 custom program. Complete mode/capability semantics remain unverified. Observed standby event `72 50 03 0a 40 02 4c` carries `0240`; wake event `72 50 03 0a 00 02 0c` carries `0200`. These event final bytes match their three-byte payload sums.

Observed action-list reply `75 80 0c 02 00 00 00 00 00 00 00 00 00 00 00 03`: the reference layout treats the payload as three four-byte LE masks for no-argument, UINT16 and string actions. Masks were `00000002`, zero and zero. The advertised action's effects and encoding are not established here. Advertised bits do not validate executing an action or establish history retrieval.

Settlement event reference layout: header `73 50 09`, one byte, two LE32 values, final byte. Field meanings, lifetime and uniqueness remain unknown. Do not treat these values as calendar timestamps or globally unique synchronization keys. Actual values stay private. The event is not proof of a device history-download API.

## Legacy packet reference

Upstream six-byte current-statistics query to `fff2`: `f7 a2 00 00 a2 fd`, checksum in the penultimate position. An earlier seven-byte variant `f7 a2 00 00 a2 44 fd` differs; failed queries do not prove legacy-protocol support or absence. The original six-byte query also yielded no telemetry in the tested pre-handshake window. DeskSteps does not poll it.

An upstream legacy status packet begins `f8 a2` and requires at least 18 bytes in the retained reference decoder. Byte 2 is belt state; byte 3 speed/10 km/h; byte 4 mode; bytes 5–7 elapsed seconds BE24; bytes 8–10 distance BE24 times 10 metres; bytes 11–13 steps BE24; bytes 14 onward remain uninterpreted tail. This decoder did not validate the packet checksum and was not a verified Z1 response path.

Offline history retrieval, user-ID/system-info initialization, vendor actions, preference writes, resets and firmware operations are not established command interfaces here; no guessed frames are supplied.

## Counter/state behavior and capture boundaries

- Pause/Resume preserve live workout counters; Stop, explicit/automatic Sleep and main-power cycling reset them in attended observations.
- Bluetooth gaps during an ongoing workout preserved cumulative counters, including a two-minute test, but receiving a counter later does not establish when unobserved activity occurred.
- A paused/offline-workout snapshot was available in one reconnect and absent on an immediate unchanged-state reconnect. Stopped snapshots are not reliably repeatable.
- Connection state, workout state and snapshot availability are separate. Connection illumination is not proof of retained counters or movement.
- An empty running belt accrued elapsed time without steps/distance/calories; changed gait affected counts at unchanged speed. The sensing algorithm and precise footfall accuracy remain unestablished.
- Windows lock and several native restart/power/radio recovery cases passed finite attended tests. Windows sleep/hibernation, every competing-client condition, real midnight capture, long-counter overflow and all-day completeness remain separate unverified cases.

Endpoint capture means logging advertisement metadata, discovered GATT metadata, read results, subscribed notifications/indications, descriptor outcomes and explicit outgoing intent for this client. It is not promiscuous over-the-air sniffing, cannot see traffic exchanged solely by another client, and may omit unselected services or packets lost before callback delivery. A real radio sniffer/HCI trace has a different capture boundary and privacy requirements. Use synthetic/redacted bytes for public reports and retain original private evidence separately.

