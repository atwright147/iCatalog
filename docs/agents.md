# iCatalog development agents

Five project-scoped Codex agents are defined in [`.codex/agents/`](../.codex/agents/).
They are reusable development roles, invoked for a task. They do not run in the
background, select backlog work, or add an AI dependency to iCatalog.

| Agent name | Use for | Main area |
|---|---|---|
| `legacy_format` | Clean-room format research, byte evidence, parser fixes and malformed-input regressions | `ivc-file-research/` |
| `catalogue_sync` | SQLite migrations, transactions, revision compatibility, sync conflicts and recovery | Catalogue, engine and sync crates |
| `native_ui` | Native egui interaction, accessibility, layout and visual checks | `crates/icatalog-ui/` |
| `release_deployment` | Desktop/NAS packaging, Docker/Compose and CI validation | `packaging/`, release workflows and assigned tasks |
| `reviewer` | Independent review for crashes, data loss, compatibility, layering, tests and documentation | Read-only inspection of the assigned change |

The main conversation remains the coordinator. It chooses the relevant roles,
assigns file ownership, integrates results and owns the final workspace checks.
An area in this table is expertise, not blanket permission to edit every file there.

## Using the roles

Current local Codex clients discover standalone TOML definitions in `.codex/agents/`.
Each file supplies `name`, `description` and `developer_instructions`, following the
[official custom-agent format](https://learn.chatgpt.com/docs/agent-configuration/subagents).
Model and reasoning settings are omitted so the roles inherit the session's choices.
Permissions, tools and credentials remain subject to the parent session's controls.
The reviewer's no-edit rule is an instruction, not an independent read-only sandbox.

Start a new Codex session in this repository after adding or changing definitions
if the current session has not discovered them. Ask for an agent by its name, for example:

> Have legacy_format investigate one UNKNOWN field in the format specification.
> Return sample-byte evidence and a proposed interpretation before changing the parser.

> Implement Docker/Compose packaging with release_deployment, then have reviewer
> examine persistence, permissions and restart behaviour using disposable data.

> Have native_ui implement this dialog change while catalogue_sync independently
> reviews whether the existing command supports it. Assign disjoint files.

> Have reviewer review the current diff for crashes, data loss and missing tests.

If a runtime cannot select custom roles, the coordinator can read the corresponding
TOML and include its `developer_instructions` in a normal subagent's assignment.
Report that fallback accurately rather than claiming native role loading succeeded.
Do not add an API service or a second orchestration framework to use these briefs.

## Coordination rules

Read the root `AGENTS.md` first, then this guide and relevant layer documentation.
The format role also reads `ivc-file-research/AGENTS.md`. Current code, decisions
and the roadmap establish what exists; role files must not freeze schema/protocol
versions or promise planned features.

- Delegate when independent work or a separate review materially helps. Usually
  use one or two specialists; keep small fixes in the main conversation. Having
  five roles does not mean running five agents at once.
- Give every assignment a goal, acceptance criteria, permitted files, dependencies
  and expected checks. Resolve overlapping writes before starting implementation.
  Keep root manifests, registries, `Taskfile.yml`, shared docs and devlog with the
  coordinator unless explicitly assigned to one worker.
- Workers do not delegate further. They report cross-layer needs to the coordinator
  instead of editing another worker's files or expanding the task.
- Each worker uses `CARGO_TARGET_DIR=target/agent-<name>` for Cargo/Task commands.
  Wait for all work using a build directory to finish before removing that worker's
  generated directory, as required by the root guide.
- Use synthetic catalogues and generated/public-domain media. Role assignment
  does not authorize changing personal libraries, deploying services, publishing
  artifacts, committing or pushing. Specific user authorization can separately
  extend that scope; the coordinator handles it.
- Workers run relevant checks and report exact commands, results and skipped checks.
  The coordinator runs `task ci` after integration, plus relevant visual, native or
  packaging checks, updates matching docs and appends `log/devlog.md`.
- Obtain an independent `reviewer` pass for changes to migrations, imports, catalogue
  replacement or sync recovery. Run it after the relevant implementation settles;
  its findings do not replace tests. The coordinator resolves findings before finishing.

## Assignment and handoff

A useful brief is: “Use `<role>` for `<goal>`. Own `<files>`; treat `<other files>`
as read-only. Success means `<observable behaviour>`. Verify with `<checks>` and
report any dependency on another role.”

Implementation handoffs include changed files and behaviour, test evidence,
compatibility/recovery implications, and unresolved questions. Format findings also
include sample names and byte offsets; UI changes identify inspected screenshots;
release work identifies host/target and artifacts. Reviewer handoffs lead with
concrete findings ordered by severity, with file/line, trigger and user impact.
