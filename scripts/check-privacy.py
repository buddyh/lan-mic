#!/usr/bin/env python3
"""Check project source or a binary for accidentally embedded private details.

This is a release guard, not a complete secret detector. Review the diff too.
Public project author/repository credits and required third-party legal notices
are intentional. No network requests are made.
"""
import argparse
import ipaddress
import re
from pathlib import Path

PATTERNS = {
    'personal home path': re.compile(rb'/(?:Users|home)/[A-Za-z0-9_.-]+/'),
    'private key': re.compile(rb'-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----'),
    'GitHub credential': re.compile(rb'(?:gh[pousr]_[A-Za-z0-9]{30,}|github_pat_[A-Za-z0-9_]{40,})'),
    'personal email': re.compile(rb'[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}'),
}
IP = re.compile(rb'(?<![\d.])(?:\d{1,3}\.){3}\d{1,3}(?![\d.])')

def violations(path, binary=False):
    data = path.read_bytes()
    failures = []
    for label, pattern in PATTERNS.items():
        # Binary crates retain required upstream notices; email text alone is not
        # evidence of a leaked private address. Source email is reviewed below.
        if binary and label == 'personal email':
            continue
        if pattern.search(data):
            failures.append(label)
    for match in IP.finditer(data):
        try:
            address = ipaddress.ip_address(match.group().decode())
        except ValueError:
            continue
        if any(address in network for network in PRIVATE_NETWORKS):
            failures.append('private network address')
            break
    return failures

PRIVATE_NETWORKS = [ipaddress.ip_network(s) for s in (
    '10.0.0.0/8', '172.16.0.0/12', '192.168.0.0/16', '100.64.0.0/10'
)]

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path)
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    if args.binary:
        files = [args.binary]
    else:
        files = [p for p in root.rglob('*') if p.is_file()
                 and not any(part in {'.git', 'target', 'dist', 'THIRD_PARTY_LICENSES', '__pycache__'}
                             for part in p.relative_to(root).parts)
                 and p.name not in {'LICENSE', 'check-privacy.py', 'Cargo.lock'}]
    failures = []
    for path in files:
        for label in violations(path, bool(args.binary)):
            failures.append(f'{path.name if args.binary else path.relative_to(root)}: {label}')
    if failures:
        raise SystemExit('\n'.join(failures))
    print(f'Privacy checks passed for {len(files)} files; review still required.')

if __name__ == '__main__':
    main()
