# Bundled typefaces

Fredoka 600 and Nunito 400–800 are bundled locally in WOFF2 format.
The Latin and Latin Extended subsets cover the French interface. These files
come from Google Fonts; no font request is made to a CDN at runtime.
Both families are distributed under the SIL Open Font License 1.1.
See [`public/licenses/fonts/`](../../../public/licenses/fonts/) for the copyright
notices and licenses. Vite copies these files unchanged into `dist/licenses/fonts/`,
which Tauri embeds with the frontend assets in production builds.

Retrieved on 2026-11-15 using the Google Fonts CSS API:
`https://fonts.googleapis.com/css2?family=Fredoka:wght@600&family=Nunito:wght@400;600;700;800&display=swap`.
Nunito uses a variable font; its requested weights share one file per subset.

- `fredoka-latin-600.woff2` (16,468 bytes)
  Source: https://fonts.gstatic.com/s/fredoka/v17/X7nP4b87HvSqjb_WIi2yDCRwoQ_k7367_B-i2yQag0-mac3OLyX8EemK.woff2
  SHA-256: `b62f2898de150f019961f4884a49f83dcd2ce74375be5e0992c594ebb8f30294`

- `fredoka-latin-ext-600.woff2` (2,692 bytes)
  Source: https://fonts.gstatic.com/s/fredoka/v17/X7nP4b87HvSqjb_WIi2yDCRwoQ_k7367_B-i2yQag0-mac3OLyX8H-mKpNk.woff2
  SHA-256: `d7d22ddf9d13a2b8db2a77c4eb14f8d5b12832451540393d6091b2e67c6189c3`

- `nunito-latin-variable.woff2` (39,128 bytes)
  Source: https://fonts.gstatic.com/s/nunito/v32/XRXV3I6Li01BKofINeaB.woff2
  SHA-256: `ba344451eab25b217a165363b1982048a5e5830a0daf36577973955a04cac793`

- `nunito-latin-ext-variable.woff2` (35,588 bytes)
  Source: https://fonts.gstatic.com/s/nunito/v32/XRXV3I6Li01BKofIO-aBXso.woff2
  SHA-256: `2c8d792869818ecb253a46bc3c63c7013df7aac2f69291c3c85e5cdc94160960`

- `public/licenses/fonts/Fredoka-OFL.txt` (4,387 bytes)
  Source: https://raw.githubusercontent.com/google/fonts/main/ofl/fredoka/OFL.txt
  SHA-256: `9bbd4f5f107895d5a213579b73821fadd05105241cf4360e86050199fcfd6034`

- `public/licenses/fonts/Nunito-OFL.txt` (4,384 bytes)
  Source: https://raw.githubusercontent.com/google/fonts/main/ofl/nunito/OFL.txt
  SHA-256: `ee9491ad3ea5d390878695bd6cce49beb3cacd5c4312f8ab574b541e9f7afa21`

License files retain the upstream text with trailing spaces removed.
Checksums above describe the files stored in this repository.
