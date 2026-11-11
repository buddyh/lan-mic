# Security and privacy

Lan Mic forwards microphone audio between a pair of trusted computers. It uses a
separate SSH connection with existing keys and strict host-key checking. It does
not open a public audio listener, contact a cloud service, record audio files, or
send analytics. The optional GitHub star prompt keeps a local usage counter; it
opens the repository only when clicked and never checks your GitHub account.

The local audio/UI sockets and state directory are restricted to the desktop user.
Programs running as that user, root, and the authenticated SSH peer are trusted.
The SSH account is a normal login account, not a sandbox. Lan Mic does not generate
keys, change host trust, enable remote login, or manage LAN Mouse certificates.

Focus currently comes from LAN Mouse's local daemon log. Treat that log and the
configuration as trusted user files. A changed log format may stop handoff; the
application should stop forwarding when focus or connectivity is uncertain.
Heartbeats and bounded audio queues limit stale forwarding but cannot guarantee
silence during every OS or driver failure. See VALIDATION.txt for tested behavior.

Diagnostics can contain computer names, input-device names, file paths, and SSH
aliases. Review and redact them before posting. Never post private keys, certificates,
microphone recordings, or complete personal configuration directories.

## Reporting a vulnerability

Use **Security → Report a vulnerability** on the project’s GitHub repository when
private reporting is enabled. If unavailable, contact the maintainer through a
private channel listed on their GitHub profile. Do not publish exploit details,
credentials, or private logs in a public issue. There is no guaranteed response SLA
for this early development preview; only the current development version is
maintained. Ordinary setup bugs can use the issue template.
