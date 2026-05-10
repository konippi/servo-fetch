# Security Policy

## Reporting a vulnerability

If you discover a security vulnerability, please report it responsibly by opening a [GitHub Security Advisory](https://github.com/konippi/servo-fetch/security/advisories/new).

Do not open a public issue for security vulnerabilities.

## Scope

servo-fetch processes untrusted web content. The following areas are in scope:

- Terminal escape injection via rendered output
- URL scheme bypass (e.g. `file://`, `javascript:`)
- SSRF via private/loopback/metadata IP addresses
- Credential leakage through URLs
- Denial of service via malicious pages

## Mitigations

- URL validation: only `http://` and `https://` schemes allowed
- SSRF protection: all private, reserved, and special-purpose IP ranges from the [IANA Special-Purpose Address Registry (RFC 6890)](https://datatracker.ietf.org/doc/html/rfc6890) are blocked, including cloud metadata endpoints
- HTTP redirects are disabled (`max_redirects: 0`) to block SSRF via redirect to a private IP after initial validation
- In-page navigation is validated: Servo `NavigationRequest`s to private or reserved hosts are denied by the navigation delegate
- Credentials are automatically stripped from URLs
- All output is sanitized to remove ANSI escape sequences, control characters, and BiDi override characters ([CVE-2021-42574](https://www.cve.org/CVERecord?id=CVE-2021-42574))
- `--js` output is sanitized before printing to the terminal
- MCP `execute_js` rejects scripts longer than 10,000 characters; MCP `fetch` output is bounded by `max_length` / `start_index` to limit response size
- HTTP API (`serve` subcommand) enforces the same SSRF protection, URL scheme whitelist, and sanitization as the CLI/MCP paths; additionally caps request bodies at 1 MiB, clamps `execute_js` expressions at 10,000 characters, and defaults to binding on `127.0.0.1` (explicit `--host` required to expose). Rate limiting, authentication, and TLS must be provided by a reverse proxy or ingress.
- The published Docker image (`ghcr.io/konippi/servo-fetch`) runs as a non-root user (UID 1001), ships with a `HEALTHCHECK`, and is signed with [cosign](https://github.com/sigstore/cosign) keyless. Release builds attach [SLSA build provenance](https://slsa.dev/) and an [SPDX SBOM](https://spdx.dev/) as OCI attestations. Recommended deployment flags: `--read-only --tmpfs /tmp --cap-drop=ALL --security-opt=no-new-privileges`.
- Crawl validates every discovered link against RFC 6890 before following it, enforces same-site (eTLD+1) scope by default, applies `robots.txt` per RFC 9309 (4xx → allow, 5xx/network → disallow, redirects disabled to avoid cross-authority SSRF), and rate-limits requests with a default 500ms interval between dispatches (MCP tool and HTTP API are bounded to this default; library and CLI callers may tune via `CrawlOptions::delay` / `--delay-ms`)

## Known limitations

- Servo's `evaluate_javascript` runs in the page context (no isolated world)
- DNS rebinding: the initial URL is validated by hostname/IP, but a hostname that resolves to a public IP during validation and a private IP at fetch time could still be reached. This is partially mitigated because HTTP redirects are disabled (see Mitigations)
- Sub-resource requests (images, scripts, iframes) loaded by the page are not subject to SSRF validation — only the initial navigation URL and in-page navigations are checked
- JavaScript executed via `--js` or `execute_js` can make secondary network requests (e.g. `fetch()`) that bypass URL validation, constrained only by the browser's Same-Origin Policy
- Password input values in the accessibility tree output are cleared, but other sensitive form data (e.g. credit card numbers in text inputs) is not filtered
- Process isolation (seccomp-bpf) is not yet implemented

## Supported versions

Security fixes are backported only to the latest 0.x minor release line. All earlier releases are considered deprecated; users are encouraged to upgrade to the latest patch version.
