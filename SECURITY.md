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
2. **Email:** Contact the maintainer directly.

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
- Miri (memory safety verification) runs on every push via CI
- `cargo-deny` checks for known vulnerabilities in dependencies
- Dependabot monitors for dependency security updates
