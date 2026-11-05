# Security

Please report vulnerabilities privately through GitHub: open the repository's **Security** tab and choose **Report a vulnerability**. Don't open a public issue for them. Include the version or commit, what you did, and what happened.

## What Spark exposes

- The server listens on `127.0.0.1` only. Every request must come from that host and port, and the UI channel, uploads, MCP tool calls and the MCP config read need the per-user token in Spark's config dir.
- Spark never sends design files anywhere. It reaches the network to download Google Fonts a design names (see `docs/DECISIONS.md`), to fetch images from http(s) URLs an agent writes into a design, and, when you enable one, to call an image provider with your own key. Keys stay in the OS keychain.
- The agent panel starts agent CLIs you have installed, with Spark's MCP attached. Some run in their non-interactive modes (`--yolo` for Gemini CLI, `--force` for Cursor) so they can act without prompts; Claude Code is only pre-approved for Spark's tools. Only start a run in a file you are happy for that agent to edit.
