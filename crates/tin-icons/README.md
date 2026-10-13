# tin-icons

Tinline's line icons, drawn by code from the mark's geometry (`docs/design/playground/src/lib/brand/MARK-MATH.md`)
and the app's smoothed corners (`docs/design/playground/src/lib/geom/GEOMETRY.md`). One source, three outputs.

```sh
cargo run -p tin-icons --bin tin-icons-gen          # rewrite generated/*
cargo run -p tin-icons --bin tin-icons-gen -- --check   # exit 1 if stale (CI)
cargo test -p tin-icons
```

| File | For | Notes |
|---|---|---|
| `generated/tin_icons.rs` | iced (desktop) | `get(name, px) -> Option<Glyph>`; `Glyph { body, string }` are SVG bodies on a 24 grid, black, with `STROKE` as the root paint. Rasterise and tint, like `ui::glyph`. |
| `generated/TinIcons.kt` | Compose (Android) | `TinIcons.Call.at(24.dp)` returns `TinGlyph(all, body, string)`. Use `all` with `Icon(tint = …)`; for a gold string, stack `body` (ink) and `string` (gold). |
| `generated/tin-icons.ts`, `docs/design/playground/src/lib/ui/tin-icons.gen.ts` | playground | `TIN[name][tinTier(px)]`; markup classes `f` (filled) and `str` (string) for the existing `.i` CSS. |
| `generated/contact-sheet.svg` | review | Every icon at 16, 20, 24 and 72 px; the string in gold. |

## Why it's built this way

- **Lines and cubics only.** Arcs in the source are turned into cubics (tolerance 0.002 grid units, tighter than kurbo's
  own SVG parser), so Compose `PathParser`, resvg and the browser draw the same curves.
- **Size tiers, like the mark.** 16, 20 and 24 px share every line; only the can mouths shrink at small sizes so they
  don't fill in. Pick the tier for the size you draw at (the helpers do this).
- **The string is its own part.** Mono everywhere by default; call buttons and connection chips may draw it gold.
- **Checked paths.** `path::Path` follows osvauld2's `runtime::frame::Path`: bounded command count, finite
  coordinates, commands only inside an open subpath. Tests also keep every icon inside the 24 box at every tier,
  check that the right can mirrors the left, that the string sags exactly `s`, and that corners match `shape.ts`
  byte for byte (`tests/fixtures/shape.txt`, regenerate with node from the playground's `shape.ts`).

Not wired into the apps yet: `icons.ts`, `pg_icons.rs`, `ui.rs` icons and Android's Material Rounded icons are
replaced screen by screen once the parity and sign-up sessions release those files.
