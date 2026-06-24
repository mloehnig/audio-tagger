# Vendored dependency

This crate is vendored (copied in-tree) so the project has no external git dependencies.

- **Source:** https://github.com/Marekkon5/SongRec (a library fork of
  https://github.com/marin-m/SongRec)
- **Commit:** `7b964ba88dccbc5380dcff274fcdcce023330239`
- **License:** GPL-3.0+ (see `LICENSE`) — original author: marin-m.

Only the audio-fingerprinting library parts are used by OneTagger
(`SignatureGenerator::make_signature_from_buffer`, `recognize_song_from_signature`).
To update, re-copy from the upstream repo at the desired commit.
