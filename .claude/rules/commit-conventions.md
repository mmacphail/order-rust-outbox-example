# Commit Message Conventions

All commit messages **must** follow the [Conventional Commits](https://www.conventionalcommits.org/) specification. This is enforced automatically on every pull request via the `commit-lint` GitHub Actions workflow.

**Format:** `<type>[optional scope]: <description>`

| Type | When to use |
|---|---|
| `feat` | A new feature |
| `fix` | A bug fix |
| `docs` | Documentation changes only |
| `style` | Formatting, missing semicolons, etc. (no logic change) |
| `refactor` | Code change that is neither a bug fix nor a new feature |
| `perf` | Performance improvement |
| `test` | Adding or updating tests |
| `build` | Changes to build system or external dependencies |
| `ci` | Changes to CI/CD configuration files |
| `chore` | Other changes that don't modify src or test files |
| `revert` | Reverts a previous commit |

**Examples:**
```
feat(orders): add pagination to GET /orders endpoint
fix(outbox): ensure event is written in same transaction
docs: update README with local setup instructions
chore: upgrade Rust toolchain to stable 1.78
ci: add commit message linting workflow
```

Breaking changes must be indicated by appending `!` after the type/scope or including a `BREAKING CHANGE:` footer.
