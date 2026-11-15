# Security policy

## Supported versions

Security fixes are applied to the latest released version of Papier.

## Reporting a vulnerability

Please do not open a public issue for a suspected vulnerability. Use GitHub's private
[security advisory form](https://github.com/noeLienard/papier/security/advisories/new) instead.

Include a minimal reproduction, the affected version and the impact you observed. You should receive an
acknowledgement within seven days. We will coordinate disclosure and credit with you after a fix is available.

## Security model

Papier does not execute scripts or fetch remote resources. Local images, fonts and stylesheets are restricted to
`baseDir`, including through symbolic links. Consumers should still set appropriate input-size and execution-time
limits when accepting documents from untrusted users.
