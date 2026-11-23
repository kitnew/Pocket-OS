# GitHub Issues workflow

[GitHub Issues](https://github.com/kitnew/Pocket-OS/issues) are the project's backlog of
**actionable work**. The rest of the repository records durable knowledge:

| Where | What |
|---|---|
| `docs/decisions/` (ADRs) | Accepted architectural decisions and why |
| `docs/architecture/`, `docs/development/` | How the system works and how to work on it |
| `docs/validation/` | What was verified, with evidence |
| Issues | Work still to be done: bugs, missing functionality, confirmed technical debt, hardware problems, research, upstream dependencies |

An Issue is a task record, not an ADR. If an Issue leads to an architectural decision,
record the decision in an ADR and link it from the Issue.

## Conventions

1. **Search first** (open and closed) before creating an Issue.
2. **Create an Issue for a real, unresolved problem**, using the *Bug report* or *Feature
   / improvement* template. No Issues for small code changes made right away, for work
   already done, or for speculative ideas without a concrete problem.
3. **Link Issues to the work**: `Refs #N` or `Fixes #N` in commit messages or PR
   descriptions, and Issue links in the docs where a limitation is described.
4. **Close with evidence**: the commit, the test, or the validation report section that
   shows it is fixed. Use the close reasons *completed* / *not planned* / *duplicate*
   (no labels for these).
5. **Blocked Issues name the blocker**, for example the upstream project and the missing
   change. Update the Issue when the blocker changes.
6. Prioritise when filing. Re-prioritise when plans change.

## Labels

Kept deliberately small. Use one type and one priority, plus areas as needed.

| Kind | Labels |
|---|---|
| Type | `bug`, `enhancement`, `technical-debt`, `research`, `documentation` |
| Priority | `P0` (breaks the device or session), `P1` (next), `P2` (normal), `P3` (low) |
| Area | `core`, `shell`, `dock`, `platform`, `hardware`, `build` |
| Status | `blocked` (an external dependency, named in the Issue) |

`accessibility` is kept for barriers that affect people with disabilities. Open/closed
state, assignees and close reasons cover the rest of the workflow.

Templates: `.github/ISSUE_TEMPLATE/bug.yml` (description, steps, expected, actual,
environment, evidence, acceptance criteria) and `feature.yml` (problem, desired behavior,
scope, acceptance criteria, dependencies). Blank Issues are allowed for quick notes but
should be completed before they are prioritised.
