# Plan

## Done
- G1 core: detect.rs (003), filter.rs (002), pdf.rs (001).
- G2 toolchain + skeleton: 006 install, 007 Flutter app + FRB bridge running on burvy's Pixel 9.

## Done: G3 app
- 008 bridge: file-path API + EXIF orientation (OpenCode), running alongside 009
- 009 camera capture + page list: reorder, retake, delete (Sonnet)
- 010 crop handles + filter picker (after 008 and 009)
- 011 export PDF + share: done
- 004 detection on light backgrounds: done (10/12 within 2%)
- 005 filter tuning: done; 014 B&W drops ruled lines: done (no highlighter to test; faint rule stubs accepted)
- 015 keep the current scan across app restarts + New scan menu: done
- 013 big zoomable page view + original toggle + retake: done

## Focus now: G3b polish from burvy's first real scan
- 016 swipe page row: delete/retake: done; 020 obvious New scan: done
- 017 last-shot thumbnail + count while scanning: done
- 018 swipe between pages in viewer + crop: done, then 019 filter apply-to-all + default: done

## Also running: G4 groundwork
- 023 AdMob banner + UMP consent with test IDs DONE (Sonnet2; fixed AdSize.banner, Impeller stays on)

## Next groups
- G3c: 021 past scans DONE; 024 test fix DONE; 022 draw on pages (pen + white-out), in progress (OpenCode); 025 orphan-file sweep (proposed, cheap).
- G4 ads: single AdMob banner + UMP consent (13+ audience).
- G5 release: app icon/name, signing, privacy policy, Play Console walkthrough with burvy.
