# Beads scaffold for Qartez indexing improvements

This folder intentionally contains **scaffold files**, not a pre-generated Beads/Dolt runtime database.

Why:
- Beads uses a Dolt-backed store that is normally initialized per-project with `bd init`.
- Embedded mode creates runtime state under `.beads/embeddeddolt/`.
- Server mode creates runtime state under `.beads/dolt/`.

So this scaffold gives you the durable bits that belong in git:
- formulas
- seed manifest
- bootstrap scripts
- planning docs

## Fast path

```bash
bd init --quiet
python3 .beads/scripts/seed_high_roi.py
bd ready
```

## Formula path

If your local Beads setup loads project-local formulas from `.beads/formulas/`, you can also use:

```bash
bd formula list
bd cook qartez-indexing-high-roi
bd mol pour qartez-indexing-high-roi
```
