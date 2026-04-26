# Security Policy

## Supported versions

| Version | Supported |
| ------- | --------- |
| 0.1.x   | ✅        |

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
- Credentials are automatically stripped from URLs
- All output is sanitized to remove ANSI escape sequences, control characters, and BiDi override characters (CVE-2021-42574)
- `--js` output is sanitized before printing to the terminal

## Known limitations

- Servo's `evaluate_javascript` runs in the page context (no isolated world)
- DNS rebinding attacks are not mitigated at the URL validation layer
- Sub-resource requests (images, scripts, iframes) loaded by the page are not subject to SSRF validation — only the initial navigation URL is checked
- JavaScript executed via `--js` or `execute_js` can make secondary network requests (e.g. `fetch()`) that bypass URL validation, constrained only by the browser's Same-Origin Policy
- Process isolation (seccomp-bpf) is not yet implemented
