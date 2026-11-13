# plugin-agent-backstage-yarn

Yarn / Backstage CLI agent launcher plugin for Basalt.

Exposes yarn scripts from the workspace root `package.json` as Basalt review
actions, plus an agent session that parses yarn/Jest output.

## Provides
- `agent:backstage-yarn`

## Review actions

Actions are only advertised when the corresponding script exists in the root
`package.json`:

| Action id | Script | Kind | Plan |
|---|---|---|---|
| `yarn-test` | `test` | Test | `CI=true yarn test --watchAll=false --passWithNoTests` |
| `yarn-build` | `build` | Build | `yarn build` |
| `yarn-lint-all` | `lint:all` | Lint | `yarn lint:all` |
| `yarn-lint` | `lint` (only when git is available) | Lint | `yarn lint` |

Notes:

- **No `build` script means no build action.** Stock Backstage apps define
  `build:all` / `build:backend`, not `build`. In that case Basalt core's native
  `tsc --noEmit` fallback handles the check lane, which is faster and produces
  structured diagnostics.
- **Tests are non-interactive.** Backstage's `repo test` defaults to watch mode
  and, in a `.git`-less shadow workspace, falls back to `--watchAll` across the
  whole monorepo. The plan sets `CI=true` and `--watchAll=false`.
- **`lint:all` is preferred over `lint`.** The stock Backstage `lint` script is
  `backstage-cli repo lint --since origin/<branch>`, which needs a git checkout.
  `yarn-lint` is only advertised when a git repository is detected.

## Output parsing

`basalt_review_action_parse_line` converts tool output into structured events:

- Jest/yarn: `PASS` / `FAIL` lines become test results.
- TypeScript: `file(line,col): error TSxxxx: message` and the pretty
  `file:line:col - error TSxxxx: message` form become diagnostics.
- ESLint: the file path line plus `line:col  error|warning  message` lines
  become diagnostics.

## Settings
- `yarn_executable` — path to yarn binary (default: `yarn`)
- `node_executable` — path to node binary (default: `node`)

## Activation
- `package.json`, `**/package.json`

## ABI

The plugin targets the current `basalt-plugin-sdk` review-action ABI:

| Export | Signature |
|---|---|
| `basalt_review_actions` | `(root_ptr, root_len, workspace_ptr, workspace_len: i32) -> u64` |
| `basalt_review_action_plan` | `(action_ptr, action_len, root_ptr, root_len, workspace_ptr, workspace_len: i32) -> u64` |
| `basalt_review_action_parse_line` | `(action_ptr, action_len, line_ptr, line_len, state_ptr, state_len: i32) -> u64` |
| `basalt_agent_parse_line` | `(line_ptr, line_len, state_ptr, state_len: i32) -> u64` |

Script discovery uses the host-provided `basalt_read_file` import.
