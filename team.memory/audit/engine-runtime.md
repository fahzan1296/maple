# Audit runtime engine (maple-tui crate) — lane audit-engine (rose)

Sumber: laporan swarm rose, 2026-11-18. Tidak ada file diubah lane ini.
Status verifikasi: temuan dibaca ulang koordinator (path:line) pada commit `319b22d`.

## Blocker
Engine crate `Tui`/`NativeBackend`/`native/` belum ter-wire ke produksi. Runtime aktual dan profil memori/CPU belum bisa diverifikasi. Temuan di bawah analisis statis.

## High
- `reveal_n`/`scroll_n` unbounded: `crates/maple-tui/src/native/state.rs:1082,1423,1430`.
- Multi-copy per frame + flush per baris: `crates/maple-tui/src/screen.rs:84-113,125,141`; `crates/maple-tui/src/tui.rs:626,652,666`; `crates/maple-tui/src/terminal/process.rs:180`.
- `ToolCard::render_at` bypass cache: `crates/maple-tui/src/render/tool_card.rs:424,445-455`.
- `OutputPane` clone string memo: `crates/maple-tui/src/render/output_pane.rs:688-696`.

## Medium
- Dead code produksi: `RenderCache`, 4 modul tak terpakai, 43 `pub fn` tanpa referensi.
- `ops.clone()`: `crates/maple-tui/src/native/state.rs:1230`.
- Overlay/emergency O(n): `crates/maple-tui/src/tui.rs:789`; `crates/maple-tui/src/render/transcript_container.rs:2292`.
- Regex dibuat per panggilan: `crates/maple-tui/src/render/render_utils.rs:2304`.
- Duplikasi root `src/` vs crate `crates/maple-tui/src/` (lihat audit-modular).

## Diff renderer
- Row-level string equality: `crates/maple-tui/src/screen.rs:157-162`.
- Tanpa bounding box kolom.
- Bracket `PAINT_BEGIN/END`: `crates/maple-tui/src/screen.rs:12-15`.
- Cap escape 4096 byte/baris: `crates/maple-tui/src/screen.rs:20`.

## Prioritas rekomendasi
1. Prune `reveal_n`/`scroll_n`.
2. Batch output + diff kolom.
3. Konsolidasi dead code + modul >5 file + duplikasi root/crate.
