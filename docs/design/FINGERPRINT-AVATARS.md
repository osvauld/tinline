# Fingerprint avatars (sacred geometry) — implementation handoff

Decided with the user on 2026-10-14. Reference lab: `docs/design/playground/src/screens/lab/AvatarLab.svelte`
(`?lab=avatars`, the "Sacred geometry: how the number builds it" panel and section 3). Login lab for where it
shows first: `?lab=accounts` (A, "Welcome back, Maya ⌄" -> "Open as").

## What and why

- Every identity's default avatar is a small **sacred-geometry mandala drawn from its key fingerprint**: lines and
  circles only, one symmetry, calm colour. Same on every device, never changes (the DID is stable; device keys
  don't affect it). Nothing is sent over the network: everyone's DID is already known.
- **No letters in the avatar.** Identities have one nickname (no first/last name), so the **full nickname sits
  beside the avatar**, like a username. Nicknames are not unique: the drawing tells two "Maya"s apart.
- A real profile picture, when set and downloaded, still replaces it (today's picture logic is unchanged).
  Group avatars keep their group icon.
- It is recognition, **not verification**. Copy must never imply it proves who someone is; the safety number does.
- Why a fingerprint: the user asked for lines and curves "based on the fingerprint — isn't a fingerprint a
  number?". Yes: 32 bytes, each part of the drawing reads its own byte, so parts vary independently and multiply.

## The number

```
fp = BLAKE3::derive_key("tinline/avatar/v1", public_key_from_did(did))   // 32 bytes
```
`identity::public_key_from_did` (crates/identity/src/did.rs) gives the 32-byte Ed25519 key. The context string is
versioned so the art can change later without touching identity. (The lab uses SHA-256 of the DID string as a
stand-in; production uses the line above and the playground port must use the same — see "Parity".)

## Byte map (one byte per part; bytes 9–31 spare)

| byte | part | rule |
|---|---|---|
| 0 | hue | `h = fp[0] / 256 * 360` degrees |
| 1 | points | `n = 5 + fp[1] % 12` (5..16) |
| 2 | star step | `kmax = (n - 1) / 2` (int); `k = 2 + fp[2] % (kmax - 1)` → star polygon {n/k} |
| 3 | ring circle radius | `cr = 8 + fp[3] % 8` |
| 4 | ring distance | `cd = 12 + fp[4] % 10` |
| 5 | inner shape | `kind = fp[5] % 3` (0 circle, 1 polygon, 2 small star); `ir = 6 + ((fp[5] >> 2) % 4) * 2` |
| 6 | extra layer | `fp[6] % 4`: 0 none, 1 Metatron lines, 2 vesica petals, 3 golden rings |
| 7 | gold layer | `fp[7] % 4`: 0 star, 1 ring, 2 inner, 3 extra |
| 8 | rotation | `rot = fp[8] / 256 * (360 / n)` degrees, about the centre |

## Geometry (100 × 100 box, centre (50, 50), angles start at the top: `a = (i/m·360 + off − 90)°`)

`pt(r, i, m, off) = (50 + r cos a, 50 + r sin a)`. `poly(r, m, step, off)`: with `g = gcd(m, step)`, draw `g`
closed sub-paths; sub-path `s` visits `pt(r, (s + j·step) % m, m, off)` for `j = 0..=m/g`. (Compound stars like
{6/2} are two triangles.)

1. **Star:** `poly(42, n, k, 0)`.
2. **Ring (Seed of Life):** `n` circles of radius `cr` centred at `pt(cd, i, n, 180/n)`, alpha 0.75.
3. **Inner:** kind 0 a circle of radius `ir`; kind 1 `poly(ir, n, 1, 180/n)`; kind 2 `poly(ir, n, max(2, (n−1)/2), 180/n)`.
4. **Extra** (alpha 0.7): 1 Metatron = `poly(cd, n, 1, 180/n)` plus a spoke from the centre to every ring centre;
   2 vesica petals = for each `i`, two arcs of radius `0.62·pr` from the centre to `pt(pr, i, n, 0)` and back
   (`pr = 0.9·cd`; SVG `A r r 0 0 1`); 3 golden rings = circles of radius `42/φ` and `42/φ²`, `φ = (1+√5)/2`.
5. Everything rotated by `rot`. Stroke round caps and joins, no fill. Gold layer strokes in the app's gold token.

**Small sizes:** below 40 px drop the ring (layer 2) and use the heavier stroke. Lab strokes: 1.8 (≥ 40 px) and
2.6 (< 40 px) in box units. Open item from review: some come out too thin at 28 px (e.g. a {14/2} star with no
extra layer reads as one circle) — tune the small stroke/what to drop, judge at 28 px in light and dark.

**Colours:** `bg = oklch(0.94 0.035 h)`, `fg = oklch(0.42 0.09 h)` in light; `bg = oklch(0.34 0.05 h)`,
`fg = oklch(0.88 0.07 h)` in dark. Fixed lightness/chroma so no avatar shouts.

## Where the code goes (Rust first: one implementation)

- **Core:** a pure module (e.g. `crates/core/src/avatar.rs`, or its own small crate if UniFFI is easier that way)
  exporting through UniFFI something like
  `avatar_art(did: String, small: bool) -> AvatarArt { strokes: Vec<Stroke>, light: Colours, dark: Colours }`,
  where a `Stroke` is a polyline in box units `{ points: Vec<f32> /* x,y pairs */, closed: bool, gold: bool, alpha: f32 }`.
  Sample circles and arcs into points in Rust (enough segments to look round at 96 px) so both apps only draw
  polylines. Do OKLCH → sRGB in Rust and return ARGB `u32`s. Bad DID → `None`/error, the app falls back to its
  current avatar.
- **Desktop (iced):** the person avatar helpers in `crates/desktop/src/view.rs` (`fn avatar`) and `ui.rs`
  (`avatar_colors`/`avatar_hue`) — draw the strokes with an iced `Canvas` (or a cached SVG/`glyph` built from the
  strokes; iced already renders SVG via `ui::glyph`). Cache per DID + size + theme.
- **Android (Compose):** `Avatar`/`SelfAvatar`/`PgAvatar` in `Components.kt` — draw with `Canvas`/`drawPath`;
  remember per DID + size + theme.
- **Playground:** move the maths into `src/lib/ui/fingerprint.ts` (the lab's `sacred()`, with BLAKE3 derive-key to
  match Rust — e.g. a small vendored JS BLAKE3 or precomputed fixtures) and use it in `lib/ui/Avatar.svelte` as the
  no-picture fallback for people (synthetic DIDs per seed person).
- **Text:** wherever an avatar shows a person, the full nickname is beside it (most rows already do). Short tag: never (user, 2026-10-14) — no tag helper is built; the drawing alone tells two same-named people apart.

## Parity and tests

- Rust unit tests: deterministic for a fixed DID; every parameter inside its range for 10,000 random keys; never a
  degenerate star (k ≥ 2, n ≥ 5); all points inside the box; small vs normal differ only as specified.
- **Golden file** like `crates/desktop/src/call_field_tests.rs` + `tools/call-field/golden.mjs`: a handful of
  synthetic DIDs → exact strokes from Rust; the playground port must reproduce them (node script). Same for the
  colour values.
- Screenshots: desktop and Android avatars at 96/44/28 px, light and dark, two identities with the same nickname.

## Rules

Read `AGENTS.md`. Heavy builds only through `python3 /home/abe/p2p_chat/scripts/buildlock.py --who <you> -- …`.
Synthetic identities. Don't touch the user's phone or installed apps. Non-UI logic lives in Rust. Commit on your
own branch; the lead merges.

## Implementation notes (2026-10-14)

- Built: `crates/core/src/avatar.rs` (+ `avatar_golden.txt`), `tools/avatar/golden.mjs` (playground parity), desktop `ui::fingerprint_face`,
  Android `FingerprintCache`/`drawFingerprint` in `ProfileAvatar.kt`, playground `lib/ui/fingerprint.ts` (vendored single-chunk BLAKE3).
  Circles and arcs are sampled at 48 / 24 segments per turn in Rust; the apps only draw polylines.
- Short tag: never (user, 2026-10-14). No tag helper exists. Full nickname beside the avatar is what existing rows already show.
- 28 px tuning (step F): compared small stroke 2.6 / 3.4 / 3.8 with the ring dropped, and the ring kept at 2.6 / 3.4, over 36 synthetic
  identities in light and dark. Ring kept turns into a smudge at 28 px; 2.6 left thin stars and bare circles faint; 3.8 starts to
  fill the inner shape. **Chosen: ring still dropped below 40 px, small stroke 3.4** (was 2.6). A {14/2}-with-no-extra avatar is still
  mostly a ring plus a small inner shape, which is its true drawing; it is told apart by hue and star count rather than by detail.
- Not verified: desktop and Android were compiled (`cargo check -p desktop`, `compileDebugKotlin`) but not run or screenshotted.

## Groups (decided 2026-10-14)

- A group's mandala comes from its **group id** (`group::record::group_id` = BLAKE3(domain ‖ owner DID ‖ random 16-byte
  nonce), made once at Genesis, permanent, known to every member): `fp = BLAKE3::derive_key("tinline/avatar/v1/group",
  utf8(gid))`, then the same byte map and geometry as a person. Nothing new is shared or sent.
- **One marker** tells a group from a person: the **rounded-square frame** (corner radius 28% of the size); people stay
  circles. Inside, the same rules as people (no extra forced layer). Works at 28 px and without colour.
- Playground: `lib/ui/fingerprint.ts` `groupFingerprint` / `groupAvatarArt`; lab `?lab=avatars` section 4.
- Apps (to do): a core export like `group_avatar_art(gid, small)`; draw it in a rounded square wherever a group's avatar
  shows (chat list, Groups tab, group header and info, group call rows in Calls), replacing the people icon.
