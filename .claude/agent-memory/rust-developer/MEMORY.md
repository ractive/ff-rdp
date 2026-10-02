# Agent Memory Index

- [project_ff_rdp_core.md](project_ff_rdp_core.md) — ff-rdp-core implementation status, transport design, testing approach
- [project_rdp_viewport_protocol.md](project_rdp_viewport_protocol.md) — No RDP actor sets viewport size; use CSS constraint approach for responsive simulation
- [Stateless CLI](project_stateless_cli.md) — daemon removed 2026-10-02 (DEC-056): one connection per command, refs stamped in-page, --throttle/--block on navigate/reload
- [project_ff_rdp_registry.md](project_ff_rdp_registry.md) — Actor Registry + Front lifecycle (iter-61p): ActorId as Arc<str>, DashMap registry, call_with_refresh helper
- [project_xtask_discipline_gates.md](project_xtask_discipline_gates.md) — check-iteration-ready aggregator and find-iteration-plan resolver (iter-75b)
- [project_serde_json_ordering.md](project_serde_json_ordering.md) — preserve_order enabled workspace-wide; text-table columns follow JSON insertion order now
- [project_flaky_redact_tests.md](project_flaky_redact_tests.md) — transport::tests::redact_* race under narrow `cargo test -- filter`; pre-existing, not a regression
- [Windows socket timeout readback](project_windows_socket_timeout_readback.md) — `write_timeout()` reads back None on Windows; assert against a baseline, never a literal
