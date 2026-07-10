# Agent Memory Index

- [project_ff_rdp_core.md](project_ff_rdp_core.md) — ff-rdp-core implementation status, transport design, testing approach
- [project_rdp_viewport_protocol.md](project_rdp_viewport_protocol.md) — No RDP actor sets viewport size; use CSS constraint approach for responsive simulation
- [project_daemon_architecture.md](project_daemon_architecture.md) — Daemon architecture review: missing watchTargets, double-boundary bug, no Front cache, no protocol version
- [project_ff_rdp_registry.md](project_ff_rdp_registry.md) — Actor Registry + Front lifecycle (iter-61p): ActorId as Arc<str>, DashMap registry, call_with_refresh helper
- [project_xtask_discipline_gates.md](project_xtask_discipline_gates.md) — check-iteration-ready aggregator and find-iteration-plan resolver (iter-75b)
- [project_serde_json_ordering.md](project_serde_json_ordering.md) — preserve_order enabled workspace-wide; text-table columns follow JSON insertion order now
- [project_flaky_redact_tests.md](project_flaky_redact_tests.md) — transport::tests::redact_* race under narrow `cargo test -- filter`; pre-existing, not a regression
- [project_console_actor_cache_gap.md](project_console_actor_cache_gap.md) — console command never starts WebConsoleActor listeners before reading cache; live_console_printf_e2e left red, needs product fix in own PR
