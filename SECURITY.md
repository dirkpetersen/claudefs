# Security Policy

## Reporting Security Vulnerabilities

ClaudeFS takes security seriously. If you discover a security vulnerability, please report it responsibly to help us protect our users.

### Reporting Process

1. **Do NOT open public GitHub issues** for security vulnerabilities. This could expose the vulnerability to malicious actors.

2. **Use GitHub Security Advisory** — Go to the [Security tab](../../security/advisories) and click "Report a vulnerability" to report privately.

3. **Alternative: Email** — If you cannot use GitHub's security advisory feature, email details to the project maintainers with:
   - Description of the vulnerability
   - Affected component(s) or version(s)
   - Steps to reproduce (if applicable)
   - Potential impact
   - Suggested fix (if you have one)

### Response Timeline

We aim to respond to security reports within 24 hours. Based on severity:

- **CRITICAL (CVSS 9.0-10.0):** Patch released within 24-48 hours
- **HIGH (CVSS 7.0-8.9):** Patch released within 1 week
- **MEDIUM (CVSS 4.0-6.9):** Patch released within 2-4 weeks
- **LOW (CVSS 0.1-3.9):** Addressed in next regular release

### Security Practices

ClaudeFS follows these security practices:

1. **Automated Dependency Scanning**
   - Daily CVE checks via `cargo audit`
   - License compliance verification
   - SBOM (Software Bill of Materials) generation
   - CI/CD integration with automatic blocking of HIGH/CRITICAL vulnerabilities

2. **Code Review**
   - All `unsafe` code is reviewed and audited (see [unsafe code policy](docs/language.md))
   - Security-sensitive modules (crypto, authentication, transport) undergo additional review
   - Fuzzing for network protocol and FUSE interface parsing

3. **Testing**
   - Unit tests for all cryptographic operations
   - Integration tests for distributed scenarios (Jepsen for linearizability)
   - POSIX compliance testing to catch edge cases

4. **Dependency Management**
   - Minimal external dependencies (embedded KV engine, custom transport)
   - Transitive dependencies audited via `cargo-deny`
   - Banned licenses (GPL, AGPL) prevented via policy

## Security Advisories

We publish security advisories on:
- GitHub Security Advisories: https://github.com/dirkpetersen/claudefs/security/advisories
- CVSS scores and impact assessment included
- Upgrade recommendations provided

## Known Limitations

ClaudeFS Phase 3 (current development) has the following known limitations:

- **Multi-tenancy:** Not yet enforced; quotas/ACLs are policy-only
- **Audit trail:** Logged but not cryptographically signed
- **Compliance (WORM):** Enforced at write time, but retention not yet validated

These are being addressed in Phase 3 (see [PHASE3_A11_INFRASTRUCTURE.md](PHASE3_A11_INFRASTRUCTURE.md)).

## Security Contact

For security issues: Use GitHub Security Advisory or contact the project maintainers privately.

For general questions about security: Open a [GitHub Discussion](../../discussions).

## References

- [Rust Security Guidelines](https://anssi-fr.github.io/rust-guide/)
- [OWASP Top 10](https://owasp.org/www-project-top-ten/)
- [CVSS Calculator](https://www.first.org/cvss/calculator/3.1)
