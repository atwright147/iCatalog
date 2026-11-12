# Third-party source attribution

iCatalog's legacy catalogue research remains clean-room. No iView, Expression
Media or Phase One source code or assets are included.

| Component | Origin | Licence | Local use |
|---|---|---|---|
| `tiny_http` 0.12.0 | [tiny-http/tiny-http](https://github.com/tiny-http/tiny-http), crates.io release 0.12.0 | MIT OR Apache-2.0; both licence texts retained in `vendor/tiny_http/` | Vendored HTTP server dependency, patched for bounded rejected-request handling and fallible Rustls TLS configuration. See [patch notes](vendor/tiny_http/ICATALOG-PATCH.md). |

Other Rust dependencies are resolved by Cargo and retain their respective package
licences. This file records copied third-party source; it is not a complete
software bill of materials. No third-party artwork is shipped.
