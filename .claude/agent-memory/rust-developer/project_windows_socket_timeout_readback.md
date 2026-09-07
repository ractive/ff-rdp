---
name: windows-socket-timeout-readback
description: On Windows CI, TcpStream::write_timeout() reads back None regardless of what set_write_timeout installed — never assert a literal socket timeout value in a test
metadata:
  type: project
---

`std::net::TcpStream::write_timeout()` returns `None` on Windows even right after a successful
`set_write_timeout(Some(d))`. Linux and macOS report the value back faithfully. A test that
asserts a literal `Some(SOME_CONST)` therefore passes on two of the three CI platforms and fails
on `test (windows-latest)` against perfectly correct code.

**Why:** discovered 2026-09-07 on PR #243 (iteration 240) — a daemon test asserting that the
goodbye frame leaves the client socket on `CLIENT_WRITE_DEADLINE` was green locally on macOS and
red on Windows with `left: None, right: Some(10s)`. Windows' `getsockopt(SO_SNDTIMEO)` does not
round-trip what `setsockopt` accepted.

**How to apply:** when a test's subject is "this code must not change the socket's timeout",
capture a baseline with `write_timeout()` on the same socket first and assert the after-value
equals the baseline. That is true on every platform, and keeps its teeth on the ones that answer
(it still fails 3/3 on macOS against the pre-fix code). The same caution applies to
`read_timeout()`. Relatedly, Windows loopback buffers differ enough that a single large frame can
be swallowed whole where macOS/Linux block — see the comments in
`unit_240_non_reading_client_does_not_stop_the_dispatcher`, which pumps until a condition rather
than asserting on one write. Related: [[daemon-architecture]].
