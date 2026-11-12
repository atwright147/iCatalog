# iCatalog patch to tiny_http 0.12.0

Source: crates.io `tiny_http` 0.12.0, upstream
<https://github.com/tiny-http/tiny-http>. The MIT and Apache-2.0 licenses are
retained in this directory. Sources and upstream tests are copied unchanged
except for the changes below; registry metadata and examples are omitted.

- `Request::respond_and_close` sends a final response with `Connection: close`,
  then shuts down a separately cloned raw socket before dropping unread body
  readers. This avoids blocking on a refused upload or taking the TLS stream
  mutex to interrupt a blocked read. It also shuts down when writing fails.
- Request bodies are delivered lazily, including bodies of 1024 bytes or less.
  Authentication can therefore reject a request before consuming its body.
  The previous small-body eager-buffering optimization is removed.
- `EqualReader` drains into a fixed 8 KiB buffer instead of allocating the
  complete unread `Content-Length` in its destructor.
- The rustls adapter uses rustls 0.23 and rustls-pemfile 2 with an explicit ring
  provider. Malformed or missing certificate/private-key PEM returns an error;
  poisoned TLS locks are recovered instead of panicking.
- Remove unused imports/mutability and express the upstream Send/Sync assertion
  as a compile-time function assertion to keep dependency builds warning-free.

The targeted regression suite is `tests/early-rejection.rs`; the bounded drain
regression is in `src/util/equal_reader.rs`. iCatalog's sync suite also exercises
authenticated HTTP(S) refusal and recovery. The vendored dependency retains its
upstream `forbid(unsafe_code)`.

`task test:http` runs the reader unit tests and targeted regression suite with
`ssl-rustls`, using this directory's committed test-only Cargo.lock. It is part of
`task ci`; application dependency resolution still uses the root Cargo.lock.
