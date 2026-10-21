# PageSeeds Operator — public install & releases

This is the **public download surface** for the PageSeeds Operator CLI:

- Install script
- GitHub Release binaries (macOS Apple Silicon)
- Customer getting-started and free/paid tool docs
- The agent skill pack (`/pageseeds-run` and friends): see [skill-pack/README.md](./skill-pack/README.md)

**Product source** (Rust CLI + marketing site) lives in a private monorepo and is not published here.

## Install

```bash
curl -fsSL https://raw.githubusercontent.com/fstrauf/pageseeds-app/main/scripts/install-cli.sh | bash
```

Requires macOS Apple Silicon for the prebuilt binary. Then:

```bash
pageseeds-cli setup --path /path/to/your/site --yes
pageseeds-cli site-overview -i <project-id>
```

## Skills

The SEO pass is run by your coding agent from the skill pack: `/pageseeds-run` plans a quota worklist of pages to create, merge and noindex from your Search Console data, and opens one pull request. Claude Code install: `/plugin marketplace add fstrauf/pageseeds-app`, then `/plugin install pageseeds@pageseeds`. Full guide: [skill-pack/README.md](./skill-pack/README.md).

## License

- **Free forever:** desk + Search Console reads (see [docs/CLI_COMMERCIAL.md](./docs/CLI_COMMERCIAL.md))
- **Paid Operator:** research, write, fix, merge, and task lifecycle — buy at [pageseeds.com](https://www.pageseeds.com/)

```bash
pageseeds-cli license activate <key>
```

## Docs

| Doc | Purpose |
|-----|---------|
| [CLI Getting Started](./docs/CLI_GETTING_STARTED.md) | Install, setup, first desk read |
| [CLI Commercial](./docs/CLI_COMMERCIAL.md) | Free vs paid tool names (source of truth for marketing) |
| [Skill pack](./skill-pack/README.md) | Install the agent skills (Claude Code plugin or `.agents/skills`) |

## Releases

Binaries are published as GitHub Releases on **this** repo (`cli-v*` tags). Builds are produced from the private monorepo CI and uploaded here.

## Support

https://www.pageseeds.com/
