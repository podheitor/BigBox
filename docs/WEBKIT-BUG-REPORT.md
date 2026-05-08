# WebKit Bugzilla report — ready to file

**Status:** drafted, not yet filed. Account creation at https://bugs.webkit.org
requires email confirmation, so the maintainer files it manually.

- File at: https://bugs.webkit.org/enter_bug.cgi
- Product: **WebKit**
- Component: **Media**
- Version: WebKit GTK
- Severity: Normal
- Platform: PC / Linux
- Attachment: [`webkit-mse-chunked-append-repro.html`](./webkit-mse-chunked-append-repro.html)
  (~3 MB self-contained, embeds a 2.2 MB fragmented MP4 as base64 — no
  network needed). Mark it as `text/html`, "patch" = no.

---

## Summary

`SourceBuffer.appendBuffer()` silently aborts when the appended `ArrayBuffer`
is larger than ~1 MiB on WebKitGTK 2.52.x with the GStreamer MSE backend.
The same buffer is accepted when sliced into ≤ 1 MiB chunks fed sequentially
on `updateend`. No exception is thrown; the `MediaSource` simply transitions
to `readyState="ended"` and emits an `error` event a few hundred ms after
the `appendBuffer()` call.

Chromium, Firefox, and Safari accept the single large append on the same
fragmented MP4.

---

## Steps to reproduce

1. Save the attached `webkit-mse-chunked-append-repro.html` and open it in
   a WebKitGTK 2.52.x browser (Epiphany works, or any Tauri / WebKit2GTK
   embedder pointing at `file:///path/to/repro.html`).
2. Click **Run both**.

The page contains:

- A 2 233 718-byte fragmented MP4 (h264 baseline + AAC, generated with
  `ffmpeg -movflags 'frag_keyframe+empty_moov+default_base_moof' -frag_duration 1000000`)
  embedded as base64.
- A "single-append" path: one `sb.appendBuffer(fmp4)` call with the entire
  buffer.
- A "chunked-append" path: same buffer sliced into 1 MiB pieces, each
  appended after the previous `updateend`.

## Expected result

Both paths complete with `updateend` and the video element renders the
testsrc pattern with the 440 Hz tone.

## Actual result on WebKitGTK 2.52.3

- **Single-append:** `appendBuffer()` returns without throwing.
  No `updateend` is ever fired. ~200 ms later the `MediaSource` emits its
  `error` event and `readyState` becomes `"ended"`. The video element
  stays at frame 0.
- **Chunked-append:** all chunks are accepted, `updateend` fires for each,
  and the clip plays end-to-end with audio.

This was first observed in WhatsApp Web and Telegram Web video playback
inside BigBox (a Tauri 2 + WebKitGTK app — https://github.com/podheitor/BigBox),
where Service Workers hand out `blob:` URLs of multi-megabyte MP4s.
WebKit's `<video src="blob:...">` path delegates to GStreamer for whole-file
playback and produces frames but no audio (likely a separate issue tracked
through the GStreamer bridge). Re-routing the same bytes through MSE
exposed the chunk-size limit reported here.

## Test environment

- WebKitGTK: **webkit2gtk-4.1 2.52.3-1** (Manjaro/BigLinux, 2026-04)
- Linux: 7.0.3-1-MANJARO x86_64
- GStreamer: 1.24.x with `gst-plugins-{base,good,bad,libav}`
- GPU: AMD Radeon (Mesa) and NVIDIA RTX (proprietary 580.95) — both reproduce
- Compositor: KWin Wayland and X11 — both reproduce
- Reproducible from a clean profile

Not reproducible on:

- Chromium 134 (single-append PASS)
- Firefox 137 (single-append PASS)
- Safari 18 / WebKit on macOS (single-append PASS — only the GTK port
  exhibits the issue)

## Hypothesis

The 1 MiB ceiling matches the default `appendWindow` segmentation in the
`WebKitMediaSourceGStreamer` element. We suspect the GStreamer-backed
`SourceBuffer::appendBufferTimerFired` path drops the queued append when
the parsed sample size exceeds an internal buffer pool slab, without
surfacing a JS exception — the codepath that raises `MediaSource.error`
runs asynchronously and the original `appendBuffer` call has already
returned by then, so JS has no synchronous signal that the operation
failed.

A pointer to the suspected codepath:
[`Source/WebCore/Modules/mediasource/SourceBuffer.cpp`](https://github.com/WebKit/WebKit/blob/main/Source/WebCore/Modules/mediasource/SourceBuffer.cpp)
combined with
[`Source/WebCore/platform/graphics/gstreamer/mse/WebKitMediaSourceGStreamer.cpp`](https://github.com/WebKit/WebKit/blob/main/Source/WebCore/platform/graphics/gstreamer/mse/WebKitMediaSourceGStreamer.cpp).

## Workaround used in the wild

Slice every `appendBuffer()` payload to ≤ 1 MiB and drive the next chunk
from the previous `updateend`:

```js
const CHUNK = 1024 * 1024;
let off = 0;
sb.addEventListener('updateend', () => {
  if (off >= total) { ms.endOfStream(); return; }
  const end = Math.min(off + CHUNK, total);
  sb.appendBuffer(buf.slice(off, end));
  off = end;
});
sb.appendBuffer(buf.slice(0, Math.min(CHUNK, total)));
```

Implemented in BigBox at
[`src-tauri/src/commands.rs`](https://github.com/podheitor/BigBox/blob/main/src-tauri/src/commands.rs)
inside the `BLOB_VIDEO_PATCH` constant.

## Impact

Any web app feeding multi-megabyte fragmented MP4 chunks through MSE on
WebKitGTK is affected. WhatsApp Web and Telegram Web are the highest-traffic
real-world examples — they ship video attachments as a single `Blob` and
call `appendBuffer` once. Browsers based on WebKitGTK (Epiphany, GNOME Web,
and any Tauri/wxWebView/Qt-WebEngine-fallback embedder) cannot play those
videos at all without per-app workarounds.

---

## Filing checklist (manual steps)

1. Create account at https://bugs.webkit.org (email: `heitorfaria@gmail.com`),
   confirm via email.
2. Click **File a Bug** → product **WebKit** → component **Media**.
3. Paste **Summary** as the bug title.
4. Paste the body above (sections "Steps to reproduce" through "Impact")
   into the description field.
5. Attach `docs/webkit-mse-chunked-append-repro.html` (Description:
   "Self-contained reproducer with embedded fmp4").
6. CC: `philn@webkit.org`, `cgarcia@webkit.org` (current GTK Media maintainers
   per `Source/WebCore/Modules/mediasource/OWNERS` if present, otherwise leave
   blank — Bugzilla auto-CCs the component owners).
7. Submit. Expect first triage in 1–4 weeks.
