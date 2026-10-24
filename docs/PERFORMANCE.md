# Performance and storage

Measured 2026-10-23/24 in the Linux cloud container (4 vCPU Xeon @ 2.1 GHz, 15 GB RAM, virtual disk), release build,
on-disk SQLite in WAL mode with `synchronous=NORMAL` (as the app uses it). Reproduce with
`FORGE_BENCH=1 cargo test --release --lib bench_ -- --nocapture --test-threads=1`
(`apps/desktop/src-tauri/src/supervisor/tests/bench.rs`). **Not measured on Windows**: NTFS, Defender scanning and
WebView2 will change these numbers, and the handoff asks for the same run there.

## Results

| Scenario | Volume | Result | Correctness asserted at that volume |
|---|---|---|---|
| Event ingestion (agent transcript) | 200,000 events of ~250 bytes | 15,700 events/s (one transaction each); database 108 MB (~540 B/event) | duplicates from re-delivery are ignored |
| Reading a long transcript | 200,000 events | first page (500) in 0.8 ms; whole transcript in 200 pages in 287 ms | every page reached |
| Many missions | 5,000 missions, 500 "running" at restart | created in 0.9 s; listing in 9 ms (capped at 500 rows); restart recovery 94 ms | all 500 active missions are recovered, not only the listed ones |
| Code index | synthetic 20,000-file repository (the index's file limit), 55,000 symbols | built in ~0.5 s; one ranking query ~70–90 ms | the file defining a named method ranks first |
| Code index, real repositories | 54–714 files (docs/EVALUATION.md) | median build 47–614 ms; query 0.2–4 ms | — |
| Memory scan | 3,000 vault notes | first scan 3.4 s; unchanged rescan 0.24 s; a mission's context package 0.29 s | an unchanged vault is not re-read |
| Storage clean-up | 200 old missions, 100,000 events | preview 47 ms; apply 0.64 s | exactly those events removed |

Reading: none of these is near what a person generates (an agent emits tens of events per second at most). The
places that grow without bound are the transcript (`activity_event`) and check logs; see below.

## What grows, and what to do about it

| Data | Bound per item | Grows with | Clean-up |
|---|---|---|---|
| Agent transcript (`activity_event`) | engine lines ≤ 16 MiB; payloads truncated | every mission | Settings → Storage: transcripts of finished missions older than N days (previewed, confirmed, audited; a marker explains the gap) |
| Check logs `runs/<id>.log`, captured output `runs/<id>.stdout` | 5 MiB log, 16 MiB captured output, 64 KiB tail kept in the database | every check run | same clean-up; the run's result and tail stay; files no run refers to are listed too |
| Proof Packs `proofs/` | one Markdown + JSON per pack | every pack | never removed automatically (they are the record) |
| Worktrees | the repository's size | open missions | removed when a mission is merged or discarded with the usual safety checks; folders no mission refers to are listed in Settings → Storage and **never deleted** by FORGE |
| Experiments `experiments/` | per-run logs | experiments | not yet covered by the clean-up (listed in Storage) |
| Database file | — | all of the above | deleted rows are reused; the file only shrinks after `VACUUM`, which FORGE does not run automatically |

Known limit: mission listings return the 500 most recently updated missions; the Missions screen says so when the
cap is reached. Recovery and clean-up query by state and are not affected by the cap.
