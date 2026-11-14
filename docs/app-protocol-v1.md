# Superseded: app-protocol v1

This page carried the v1 MCP/app protocol description (pre-`0.3.0`, including
the retired `apps`/`api`/`exec` method index and Session) and was later reduced to
a copy of the current-protocol notice. It is now only a historical entry point,
not current guidance.

Current `dtw mcp` defaults to `apps` and mandatory-app `exec`. `dtw mcp --atomic`
selects `apps` plus fourteen camelCase operations without JavaScript. Discover the
actual tools before use. Both surfaces share object fields; atomic calls add
`app`. Current methods, input/output and limits are in the [bundled Skill API](../skills/desktop-world/API.md);
[architecture](architecture.md) records the current boundaries and
[agent-protocol-migration](agent-protocol-migration.md) records the migration from
the retired `api`/`result`/Session interface.
