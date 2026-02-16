# Security Policy

## Supported Versions

| Version | Supported |
|---------|-----------|
| 0.1.x   | Yes      |

## Reporting a Vulnerability

**Please do not open a public GitHub issue for security vulnerabilities.**

Instead, use one of these methods:

1. **GitHub Security Advisories** (preferred): Go to the
   [Security tab](https://github.com/tachyon-beep/murk/security/advisories/new)
   and create a new private advisory.
2. **GitHub profile:** Contact via GitHub: open a [security advisory](https://github.com/tachyon-beep/murk/security/advisories/new) or reach out through the [maintainer's GitHub profile](https://github.com/tachyon-beep).

### What to include

- Description of the vulnerability
- Steps to reproduce
- Affected versions
- Potential impact

### What to expect

- **Acknowledgment** within 48 hours
- **Assessment** within 1 week
- **Fix or mitigation** as soon as practical, typically within 30 days
- Credit in the release notes (unless you prefer to remain anonymous)

## Security Practices

- `#![forbid(unsafe_code)]` on all crates except `murk-arena` and `murk-ffi`
- CI runs Miri (memory safety verification) on every push
- `cargo-deny` checks for known vulnerabilities in dependencies
- Dependabot monitors for dependency security updates
