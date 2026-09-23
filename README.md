# plugin-agent-backstage-yarn

Yarn / Backstage CLI agent launcher plugin for Basalt.

Exposes yarn as a Basalt agent that can execute:
- `yarn test` — run test suite
- `yarn build` — build all packages
- `yarn lint` — lint the workspace
- `yarn backstage-cli package build` — Backstage-specific package build

## Provides
- `agent:backstage-yarn`

## Settings
- `yarn_executable` — path to yarn binary (default: `yarn`)
- `node_executable` — path to node binary (default: `node`)

## Activation
- `package.json`, `**/package.json`
