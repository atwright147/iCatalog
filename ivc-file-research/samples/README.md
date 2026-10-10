# Legacy catalogue samples

16 real iView / Expression / Phase One Media Pro catalogues, used as parser oracles by `ivc-file-research/crates/ivc/tests/samples.rs`. Read-only: never modify.

## Provenance and licence

- Samples courtesy of Tyler Thorsted's PRONOM_Research project: https://github.com/thorsted/PRONOM_Research/tree/main/Submissions/iView/Samples
- Pinned commit: `2c26686a0577028783b2f24977cdaf78e6ca39fb`.
- Licence: **CC0 1.0** (public domain), as stated in the upstream README (the repo has no formal LICENSE file). That makes the corpus safe to redistribute; the corpus (~183 KB) is committed to the repo.

## Usage (optional)

The samples are committed (CC0 1.0). `fetch.sh` + `SHA256SUMS` are retained as an optional re-fetch/verifier; run `fetch.sh` if the corpus is missing.

```
./fetch.sh          # download missing/changed files, verify against SHA256SUMS
./fetch.sh --force  # re-download everything
```

Hashes are pinned in `SHA256SUMS`; a file is moved into place only if its SHA-256 matches.

`cargo test -p ivc` uses the committed corpus; running `fetch.sh` first is not required.
