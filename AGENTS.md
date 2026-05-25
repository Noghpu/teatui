# Agent Notes

Greenfield. No backwards compat. Break shape if design says so.

Read [docs/design.md](docs/design.md) first. Design is source of truth.

Tests minimal. No regression test farms. No architecture contract tests. Test
only risky logic and parsers.

Repo uses jj. Use `jj --no-pager ...`. Do not use git history/status.

Command runner is `just`:

- Prefer `just verify` for handoff because it bundles formatting, compile
  check, linting, and tests. Use a single focused recipe only when exactly one
  check is relevant, such as formatting-only docs or a narrow compile probe.
- `just fmt`: format code.
- `just check`: compile check.
- `just clippy`: lint Rust.
- `just test`: run minimal tests.
- `just verify`: run all handoff checks.
